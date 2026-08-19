# CLI Commands

The application binary doubles as a CLI helper. These subcommands are available
in both the GUI build and the CLI-only build (`--no-default-features`).

| Command | Behavior |
| --- | --- |
| `emterm` | Launches the terminal (GUI build only) |
| `emterm markdown <file>` | Emit Markdown display sequence to stdout |
| `emterm json <file>` | Emit JSON display sequence to stdout |
| `emterm yaml <file>` | Emit YAML display sequence to stdout |
| `emterm image <file> [--protocol kitty\|sixel]` | Emit image display sequence to stdout |

Dispatch lives in `src-tauri/src/main.rs`; the subcommand implementations are in
`src-tauri/src/cli/`.

## tmux support

Inside tmux, CLI commands automatically wrap sequences in DCS passthrough
(`ESC P tmux; ... ESC \`). This requires `set -g allow-passthrough on` in the
tmux config.
