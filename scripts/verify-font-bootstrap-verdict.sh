#!/usr/bin/env bash
# Verdict helper for the un-fetched scenario of scripts/verify-font-bootstrap.sh.
#
# Function definitions only. Loading this file performs no work: nothing is
# read, written, spawned, or hooked at exit. That property is what lets
# verify-font-bootstrap-verdict.test.ts drive the real, shipped decision
# logic inside the bun CI job, which has no Rust toolchain and no network
# (NFR1, D1).
#
# See feature-docs/verify-font-bootstrap-classification/{SPEC,IMPLEMENTATION}.md.

# count_executed_tests <logfile> -> total `passed + failed` summed across
# every `test result:` line cargo printed (0 if the build never got that
# far). Relocated character-for-character from verify-font-bootstrap.sh
# (D6); its behaviour on every input is unchanged.
count_executed_tests() {
    awk '
        /^test result:/ {
            for (i = 1; i <= NF; i++) {
                if ($i == "passed;") passed += $(i - 1)
                if ($i == "failed;") failed += $(i - 1)
            }
        }
        END { print passed + failed + 0 }
    ' "$1"
}

# classify_unfetched_verdict <exit-status> <logfile> -> prints exactly one
# line on stdout: the outcome token (`pass`, `warn`, or `fail`), a single
# space, then the message (empty for `pass`). Selects the branch from the
# derived executed count first, then consults the exit status (FR1):
#
#   executed == 0, status != 0  -> fail: names a build stop
#   executed  > 0, status != 0  -> warn: tests executed and the run reported
#                                  a failure; never the build-stop wording
#   executed == 0, status == 0  -> fail: "command exited 0 but 0 tests executed"
#   executed  > 0, status == 0  -> pass: empty message
#
# No message states a test count that was not derived from the captured log
# for this very run (FR5). Always returns success: the outcome is
# communicated through the printed token, never through this function's own
# exit status (a non-success return would be indistinguishable from a
# classified failure).
classify_unfetched_verdict() {
    local status="$1"
    local logfile="$2"
    local executed
    executed=$(count_executed_tests "$logfile")

    if [ "$executed" -eq 0 ]; then
        if [ "$status" -ne 0 ]; then
            printf 'fail the build script stopped the build (exit %s), %s tests executed\n' "$status" "$executed"
        else
            printf 'fail command exited 0 but 0 tests executed\n'
        fi
    else
        if [ "$status" -ne 0 ]; then
            printf 'warn tests executed and the run reported a failure (exit %s), %s tests executed\n' "$status" "$executed"
        else
            printf 'pass \n'
        fi
    fi
    return 0
}
