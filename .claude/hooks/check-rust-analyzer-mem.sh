#!/usr/bin/env bash
# PostToolUse hook: kill any rust-analyzer process whose RSS / total
# physical memory exceeds THRESHOLD_PCT (%). Claude Code re-spawns
# rust-analyzer on the next edit, so kill-only is sufficient — we
# don't manage state across runs here.
#
# Invoked from .claude/settings.json. Reads (and ignores) Claude
# Code's PostToolUse JSON on stdin; never blocks the edit.

set -u

THRESHOLD_PCT=8.0

# Drain stdin so the parent doesn't see a SIGPIPE if it tries to write more.
cat >/dev/null 2>&1 || true

killed=0
checked=0

# pgrep -f matches the full command line; rust-analyzer ships as a
# single binary so this catches every instance regardless of launcher
# (claude code, editor, etc.). Exclude the lightweight
# `rust-analyzer-proc-macro-srv` helper — killing it just makes
# rust-analyzer respawn it on the next macro expansion, and its
# steady-state memory is tiny.
for pid in $(pgrep -f rust-analyzer 2>/dev/null); do
    cmd=$(ps -p "$pid" -o args= 2>/dev/null)
    case "$cmd" in
        *proc-macro-srv*) continue ;;
    esac
    checked=$((checked + 1))
    # %mem column = RSS / total physical memory * 100 (one decimal).
    mem=$(ps -p "$pid" -o %mem= 2>/dev/null | tr -d ' ')
    [ -z "$mem" ] && continue
    # awk float comparison — avoids depending on bc.
    if awk -v m="$mem" -v t="$THRESHOLD_PCT" 'BEGIN { exit !(m > t) }'; then
        if kill "$pid" 2>/dev/null; then
            killed=$((killed + 1))
            echo "[ra-mem-guard] killed rust-analyzer pid=$pid mem=${mem}% (> ${THRESHOLD_PCT}%)" >&2
        fi
    fi
done

if [ "$checked" -gt 0 ] && [ "$killed" -eq 0 ]; then
    # Optional: comment this out if the noise becomes annoying.
    : # silent when nothing crossed the threshold
fi

exit 0
