#!/usr/bin/env bash
# Claude Code PostToolUse hook: auto-format the file just edited.
#
# Reads the hook payload (JSON on stdin), pulls tool_input.file_path, and
# formats by extension:
#   *.rs                         -> rustfmt (style from rustfmt.toml, edition 2024)
#   *.ts *.tsx *.js *.jsx *.css  -> biome format --write (config: biome.json)
#
# Always exits 0 so a formatting hiccup never blocks the edit.
set -u

input="$(cat)"
file="$(printf '%s' "$input" | jq -r '.tool_input.file_path // empty' 2>/dev/null)"
[ -n "$file" ] || exit 0
[ -f "$file" ] || exit 0

# Repo root = two levels up from .claude/hooks/
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

case "$file" in
	*.rs)
		rustfmt --edition 2024 "$file" >/dev/null 2>&1 ;;
	*.ts | *.tsx | *.js | *.jsx | *.mjs | *.cjs | *.css)
		(cd "$root" && bunx biome format --write "$file" >/dev/null 2>&1) ;;
esac

exit 0
