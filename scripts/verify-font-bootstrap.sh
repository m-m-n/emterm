#!/usr/bin/env bash
# Bootstrap-verification script (FR6).
#
# Reproduces the three bootstrap paths of the font-bootstrap feature from
# isolated, un-fetched trees and reports one pass/fail verdict per scenario,
# aggregated into a single exit status:
#
#   un-fetched      - a fresh, empty-font tree runs the reported library-test
#                     command and must actually execute tests.
#   fetch-failure   - the same fresh tree, with the download/hash tooling the
#                     acquisition path needs made unresolvable, must stop the
#                     build with the actionable message and leave no partial
#                     font file behind.
#   already-fetched - a tree whose fonts were populated through the
#                     acquisition path itself must build without any further
#                     download and without rewriting a font file.
#
# Usage: bash scripts/verify-font-bootstrap.sh
# Invoked from the repository root, with no arguments and no required
# environment variables. Exits 0 iff every scenario passed.
#
# See feature-docs/worktree-font-bootstrap/{SPEC,IMPLEMENTATION}.md for the
# pinned contracts this script observes (the acquisition script's invocation
# contract, the opt-out switch, the actionable message's two literal lines).
#
# Isolation: each scenario runs in its own `git worktree add --detach` tree
# under a scratch directory (this script's own scratch area, outside the
# repository). The repository's primary checkout is never used as a
# scenario's build tree and its font directory is never touched. Everything
# created is removed when the script finishes, including on failure.

set -uo pipefail
# Deliberately not `set -e`: one scenario's failure must not prevent the
# others from running (every scenario reports its own verdict).

PREFIX="verify_font_bootstrap"

log() {
    printf '%s.%s\n' "$PREFIX" "$1"
}

fail_usage() {
    printf '%s.usage: %s\n' "$PREFIX" "$1" >&2
    exit 2
}

# ---------------------------------------------------------------
# Preconditions
# ---------------------------------------------------------------

if [ ! -f "scripts/fetch-fonts.sh" ] || [ ! -f "src-tauri/Cargo.toml" ]; then
    fail_usage "must be invoked from the repository root (scripts/fetch-fonts.sh and src-tauri/Cargo.toml not found relative to \$PWD)"
fi

for tool in git cargo bash mktemp find touch; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        fail_usage "required tool '$tool' not found in PATH"
    fi
done

# Resolved relative to this script's own file location, not the working
# directory, so the script behaves identically however it is invoked (D1).
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
VERDICT_HELPER="$SCRIPT_DIR/verify-font-bootstrap-verdict.sh"
if [ ! -r "$VERDICT_HELPER" ]; then
    fail_usage "verdict helper not found or unreadable at $VERDICT_HELPER"
fi
# shellcheck source=./verify-font-bootstrap-verdict.sh
source "$VERDICT_HELPER"

REPO_ROOT="$(pwd)"
SCRATCH_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/verify-font-bootstrap.XXXXXX")"

# Every git-worktree directory this run has successfully created, so cleanup
# can find them even if a scenario fails partway through.
CREATED_WORKTREES=()

cleanup() {
    local dir
    for dir in "${CREATED_WORKTREES[@]:-}"; do
        [ -n "$dir" ] || continue
        # Plain `remove` (no --force): the trees only ever gain gitignored
        # build output / fetched fonts, so `git status` stays clean and
        # plain removal succeeds. Fall back to a manual rm + prune so a
        # scenario that failed before completing still leaves nothing
        # behind (AC-6).
        if ! git -C "$REPO_ROOT" worktree remove "$dir" >/dev/null 2>&1; then
            rm -rf "$dir"
            git -C "$REPO_ROOT" worktree prune >/dev/null 2>&1 || true
        fi
    done
    rm -rf "$SCRATCH_ROOT"
}
trap cleanup EXIT

# ---------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------

# add_scenario_worktree <name> -> prints the created tree's absolute path on
# stdout, or nothing (with a message on stderr) on failure.
#
# NOTE: this is always invoked via command substitution (`tree=$(...)`) by
# its callers, which runs it in a subshell — any array mutation made here
# would be invisible to the parent shell once the subshell exits. Callers
# are responsible for appending the returned path to CREATED_WORKTREES
# themselves, in their own (non-subshell) scope.
add_scenario_worktree() {
    local name="$1"
    local dir="$SCRATCH_ROOT/$name"
    if git -C "$REPO_ROOT" worktree add --detach --quiet "$dir" HEAD \
        >"$SCRATCH_ROOT/$name.worktree-add.log" 2>&1; then
        printf '%s\n' "$dir"
        return 0
    fi
    return 1
}

# font_dir_has_only_pristine_files <font-dir> -> 0 if the directory holds
# nothing but the three tracked files (.gitignore, LICENSE, README.md) that
# ship in a font-less checkout, 1 otherwise.
font_dir_has_only_pristine_files() {
    local dir="$1"
    local extra
    extra=$(find "$dir" -mindepth 1 -maxdepth 1 -type f \
        ! -name '.gitignore' ! -name 'LICENSE' ! -name 'README.md' 2>/dev/null)
    [ -z "$extra" ]
}

# make_broken_download_tools_dir <dir> -> populates <dir> with `curl` and
# `wget` shims that always fail, so a PATH prepended with <dir> makes the
# acquisition script's download tooling unresolvable. This shims the
# tooling the acquisition script depends on, never the acquisition script
# itself (the single-acquisition-path rule forbids stubbing fetch-fonts.sh).
make_broken_download_tools_dir() {
    local dir="$1"
    mkdir -p "$dir"
    local tool
    for tool in curl wget; do
        cat >"$dir/$tool" <<EOF
#!/bin/sh
printf '%s.shim: %s disabled for the fetch-failure scenario\n' "$PREFIX" "$tool" >&2
exit 127
EOF
        chmod +x "$dir/$tool"
    done
}

# tail_log <logfile> -> the last part of a captured log, for a failing
# scenario's report (enough to diagnose without re-running the script).
tail_log() {
    printf -- '--- last 60 lines of %s ---\n' "$1"
    tail -n 60 "$1"
    printf -- '--- end ---\n'
}

RESULTS_PASS=0
RESULTS_TOTAL=0

report_pass() {
    log "$1: PASS"
    RESULTS_PASS=$((RESULTS_PASS + 1))
    RESULTS_TOTAL=$((RESULTS_TOTAL + 1))
}

report_fail() {
    local name="$1" reason="$2" logfile="${3:-}"
    log "$name: FAIL ($reason)"
    if [ -n "$logfile" ] && [ -f "$logfile" ]; then
        tail_log "$logfile" >&2
    fi
    RESULTS_TOTAL=$((RESULTS_TOTAL + 1))
}

# report_warn <name> <reason> [logfile] -> a scenario that ran to completion
# but whose result deserves attention rather than silent acceptance (D3):
# emits its own prefixed WARN line plus the captured log's tail on stderr —
# the same diagnostic report_fail produces — then counts toward the passed
# tally exactly as report_pass does, so the aggregate exit status stays
# success when the other scenarios pass (FR6, NFR5).
report_warn() {
    local name="$1" reason="$2" logfile="${3:-}"
    log "$name: WARN ($reason)"
    if [ -n "$logfile" ] && [ -f "$logfile" ]; then
        tail_log "$logfile" >&2
    fi
    report_pass "$name"
}

# ---------------------------------------------------------------
# Scenario: un-fetched
# ---------------------------------------------------------------
scenario_unfetched() {
    local name="un-fetched"
    local tree
    if ! tree=$(add_scenario_worktree "unfetched"); then
        report_fail "$name" "could not create an isolated tree" "$SCRATCH_ROOT/unfetched.worktree-add.log"
        return
    fi
    CREATED_WORKTREES+=("$tree")

    if ! font_dir_has_only_pristine_files "$tree/src-tauri/assets/fonts"; then
        report_fail "$name" "environment problem: isolated tree's font directory was not empty at scenario start"
        return
    fi

    local logfile="$SCRATCH_ROOT/unfetched.test.log"
    (
        cd "$tree" || exit 125
        # The literal reported reproduction command (FR6 / SPEC AC1).
        CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib
    ) >"$logfile" 2>&1
    local status=$?

    # AC1 of the prior feature's SPEC pins the pass condition as a non-zero
    # executed-test count, not a zero exit status: a run that executed tests
    # and reported a failure (e.g. a known parallelism-dependent flake in the
    # library suite) therefore satisfies it, but is not silently folded into
    # a plain pass — it is surfaced as a warning instead (D3).
    local outcome_line outcome message
    outcome_line=$(classify_unfetched_verdict "$status" "$logfile")
    outcome="${outcome_line%% *}"
    message="${outcome_line#* }"

    case "$outcome" in
        pass)
            report_pass "$name"
            ;;
        warn)
            report_warn "$name" "$message" "$logfile"
            ;;
        fail)
            report_fail "$name" "$message" "$logfile"
            ;;
        *)
            report_fail "$name" "internal error: verdict helper returned unexpected outcome '$outcome'" "$logfile"
            ;;
    esac
}

# ---------------------------------------------------------------
# Scenario: fetch-failure
# ---------------------------------------------------------------
scenario_fetch_failure() {
    local name="fetch-failure"
    local tree
    if ! tree=$(add_scenario_worktree "fetch-failure"); then
        report_fail "$name" "could not create an isolated tree" "$SCRATCH_ROOT/fetch-failure.worktree-add.log"
        return
    fi
    CREATED_WORKTREES+=("$tree")

    local font_dir="$tree/src-tauri/assets/fonts"
    if ! font_dir_has_only_pristine_files "$font_dir"; then
        report_fail "$name" "environment problem: isolated tree's font directory was not empty at scenario start"
        return
    fi

    local shim_dir="$SCRATCH_ROOT/fetch-failure.shims"
    make_broken_download_tools_dir "$shim_dir"

    local marker="$SCRATCH_ROOT/fetch-failure.marker"
    touch "$marker"

    local logfile="$SCRATCH_ROOT/fetch-failure.build.log"
    (
        cd "$tree" || exit 125
        # Environment manipulation only (single-acquisition-path rule):
        # the acquisition script's own download tooling is made
        # unresolvable by prepending shims ahead of it on PATH. Nothing
        # about scripts/fetch-fonts.sh itself is touched.
        PATH="$shim_dir:$PATH" \
            CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml
    ) >"$logfile" 2>&1
    local status=$?

    if [ "$status" -eq 0 ]; then
        report_fail "$name" "build succeeded despite unresolvable acquisition tooling" "$logfile"
        return
    fi

    if ! grep -qF 'build_rs.font_missing: bundled font missing at' "$logfile"; then
        report_fail "$name" "actionable message's first line not found in build output" "$logfile"
        return
    fi
    if ! grep -qF 'Run `make fetch-fonts` (or `bash scripts/fetch-fonts.sh`) to download bundled fonts.' "$logfile"; then
        report_fail "$name" "actionable message's second line not found in build output" "$logfile"
        return
    fi

    local leftovers
    leftovers=$(find "$font_dir" -newer "$marker" -type f 2>/dev/null)
    if [ -n "$leftovers" ]; then
        report_fail "$name" "font directory contains a partial or unverified file after the stop: $leftovers" "$logfile"
        return
    fi

    report_pass "$name"
}

# ---------------------------------------------------------------
# Scenario: already-fetched
# ---------------------------------------------------------------
scenario_already_fetched() {
    local name="already-fetched"
    local tree
    if ! tree=$(add_scenario_worktree "already-fetched"); then
        report_fail "$name" "could not create an isolated tree" "$SCRATCH_ROOT/already-fetched.worktree-add.log"
        return
    fi
    CREATED_WORKTREES+=("$tree")

    local font_dir="$tree/src-tauri/assets/fonts"
    local populate_log="$SCRATCH_ROOT/already-fetched.populate.log"
    # Populate through the acquisition path itself (D6) — never by copying
    # bytes from the primary checkout.
    if ! DEST_DIR="$font_dir" bash "$tree/scripts/fetch-fonts.sh" >"$populate_log" 2>&1; then
        report_fail "$name" "could not populate fonts through the acquisition path (environment problem, e.g. no network)" "$populate_log"
        return
    fi

    local marker="$SCRATCH_ROOT/already-fetched.marker"
    touch "$marker"

    local build_log="$SCRATCH_ROOT/already-fetched.build.log"
    (
        cd "$tree" || exit 125
        CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml
    ) >"$build_log" 2>&1
    local status=$?
    if [ "$status" -ne 0 ]; then
        report_fail "$name" "build failed on an already-fetched tree (exit $status)" "$build_log"
        return
    fi

    # Strongest available corroboration (no network-isolation facility
    # exists): re-run the sole acquisition path and require it to report
    # every file already up to date, never a download.
    local recheck_log="$SCRATCH_ROOT/already-fetched.recheck.log"
    if ! DEST_DIR="$font_dir" bash "$tree/scripts/fetch-fonts.sh" >"$recheck_log" 2>&1; then
        report_fail "$name" "acquisition path failed on its post-build up-to-date check" "$recheck_log"
        return
    fi
    if grep -qi 'downloading' "$recheck_log"; then
        report_fail "$name" "acquisition path reported a download after the measured build" "$recheck_log"
        return
    fi

    local touched
    touched=$(find "$font_dir" -newer "$marker" -type f 2>/dev/null)
    if [ -n "$touched" ]; then
        report_fail "$name" "font file rewritten during the measured build: $touched" "$build_log"
        return
    fi

    report_pass "$name"
}

# ---------------------------------------------------------------
# Run all scenarios independently, then aggregate.
# ---------------------------------------------------------------

scenario_unfetched
scenario_fetch_failure
scenario_already_fetched

log "summary: $RESULTS_PASS/$RESULTS_TOTAL scenarios passed"

if [ "$RESULTS_PASS" -eq "$RESULTS_TOTAL" ]; then
    exit 0
fi
exit 1
