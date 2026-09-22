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
# far). The counting program itself is relocated character-for-character
# from verify-font-bootstrap.sh (D6); its behaviour on every input is
# unchanged.
#
# Reports failure (non-zero return) and prints no count when <logfile>
# cannot be read (absent, or permission-denied) (AC-6, D8). When it can be
# read, its bytes are handed to the counting program through standard
# input rather than as a positional argument, so a path beginning with `-`
# or containing `=` is never at risk of being reinterpreted as an option or
# a variable assignment (AC-8, D8).
count_executed_tests() {
    local logfile="$1"
    if [ ! -r "$logfile" ]; then
        return 1
    fi
    awk '
        /^test result:/ {
            for (i = 1; i <= NF; i++) {
                if ($i == "passed;") passed += $(i - 1)
                if ($i == "failed;") failed += $(i - 1)
            }
        }
        END { print passed + failed + 0 }
    ' < "$logfile"
}

# classify_unfetched_verdict <exit-status> <logfile> -> prints exactly one
# line on stdout: the outcome token (`pass`, `warn`, or `fail`), a single
# space, then the message (empty for `pass`). Selects the branch from the
# derived executed count first, then consults the exit status (FR1):
#
#   count not derivable          -> fail: names the log path or the status
#                                  argument that could not be interpreted;
#                                  states no test count (AC-1..AC-5, D8)
#   executed == 0, status != 0  -> fail: names a build stop
#   executed  > 0, status != 0  -> warn: tests executed and the run reported
#                                  a failure; never the build-stop wording
#   executed == 0, status == 0  -> fail: "command exited 0 but 0 tests executed"
#   executed  > 0, status == 0  -> pass: empty message
#
# Neither argument is trusted to be well-formed. Two guards run before the
# table above is consulted, in this order (D8):
#   1. The executed count must have been derived: count_executed_tests must
#      have reported success AND printed a non-negative integer. An
#      unreadable/absent log fails this guard.
#   2. The status argument must be interpretable as an integer.
# A value that was never derived never selects a classification branch — it
# always yields `fail`, never `pass` and never `warn` (AC-1..AC-5).
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

    if ! executed=$(count_executed_tests "$logfile") || ! [[ "$executed" =~ ^[0-9]+$ ]]; then
        printf 'fail could not derive the executed test count from %s\n' "$logfile"
        return 0
    fi

    if ! [[ "$status" =~ ^-?[0-9]+$ ]]; then
        printf 'fail exit status %s is not an integer\n' "$status"
        return 0
    fi

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
