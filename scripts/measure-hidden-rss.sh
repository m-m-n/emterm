#!/bin/bash
#
# Sample VmRSS for the eMterm backend at a fixed interval and write a
# CSV trace under tmp/. Used by TS-26 / NFR3 (hidden 1-hour backend RSS
# growth) — the operator runs this in a side terminal, hides the eMterm
# window, leaves it idle for >= 60 minutes, then aborts with Ctrl-C.
#
# Usage:
#   ./scripts/measure-hidden-rss.sh <pid> [interval_seconds]
#
# Example:
#   pid=$(pgrep -f 'src-tauri/target/.*emterm$' | head -1)
#   ./scripts/measure-hidden-rss.sh "$pid" 60
#
# Output:
#   tmp/hidden-rss-<pid>-<unix_ts>.csv
#   columns: unix_ts,iso_ts,vm_rss_kib

set -e

PID="${1:-}"
INTERVAL="${2:-60}"

if [ -z "$PID" ]; then
  echo "usage: $0 <pid> [interval_seconds]" >&2
  exit 2
fi

if [ ! -d "/proc/$PID" ]; then
  echo "error: pid $PID not found" >&2
  exit 1
fi

mkdir -p tmp
START_TS="$(date +%s)"
OUT="tmp/hidden-rss-${PID}-${START_TS}.csv"
echo "unix_ts,iso_ts,vm_rss_kib" > "$OUT"
echo "Recording VmRSS for pid $PID every ${INTERVAL}s -> $OUT"
echo "Hide the eMterm window now and leave it hidden for >= 1 hour."
echo "Press Ctrl-C to stop sampling."

trap 'echo; echo "stopped. log: $OUT"; exit 0' INT TERM

while true; do
  if [ ! -d "/proc/$PID" ]; then
    echo "process $PID exited; stopping" >&2
    exit 0
  fi
  RSS_KIB="$(awk '/^VmRSS:/ {print $2}' /proc/"$PID"/status 2>/dev/null || echo "")"
  if [ -z "$RSS_KIB" ]; then
    RSS_KIB=-1
  fi
  TS="$(date +%s)"
  ISO="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf "%s,%s,%s\n" "$TS" "$ISO" "$RSS_KIB" >> "$OUT"
  sleep "$INTERVAL"
done
