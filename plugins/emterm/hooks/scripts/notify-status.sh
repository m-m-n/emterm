#!/bin/sh
# Claude Code hook script: reports Claude Code lifecycle state via the
# terminalSequence JSON hook-output field (SPEC.md FR1-FR3 in this plugin's
# feature docs).
#
# No subprocess, no file open, no timer: the OSC 777 agent-status sequence is
# built inline and printed once. POSIX sh only — no bashisms.
#
# Behaviour, in order (silent rejection on every invalid path, NFR4):
#   1. Not exactly one positional argument -> exit 0, no output.
#   2. Argument not one of idle/working/blocked/done -> exit 0, no output.
#   3. Otherwise: print one line of JSON {"terminalSequence": "<sequence>"}.

if [ "$#" -ne 1 ]; then
    exit 0
fi

case "$1" in
    idle|working|blocked|done)
        ;;
    *)
        exit 0
        ;;
esac

printf '{"terminalSequence":"\\u001b]777;emterm;agent-status;v=1;state=%s;name=claude-code\\u001b\\\\"}\n' "$1"

exit 0
