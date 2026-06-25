# Bundled Fonts

These fonts are bundled with `emterm` and embedded via `include_bytes!` at
build time. They are **not** tracked in git — populate this directory by
running:

```bash
make fetch-fonts            # or: bash scripts/fetch-fonts.sh
```

`make setup`, `make dev`, `make build`, `make win-build`, and `make dpkg`
depend on `fetch-fonts`, so a fresh clone needs no extra step. The
CLI-only targets (`make cli-build`, `make cli-dpkg`) deliberately skip
the fetch step — they do not embed any fonts.

All bundled fonts are distributed under the SIL Open Font License 1.1;
see `LICENSE` in this directory.

## Inventory

| File | Upstream | SHA-256 |
|---|---|---|
| `NotoColorEmoji.ttf` | <https://github.com/googlefonts/noto-emoji> | `ede3ac600162ca750e2aba610285f06f30d8c0dd198bcec1d53cb6efe660306c` |
| `NotoSansCJKjp-Regular.otf` | <https://github.com/notofonts/noto-cjk> | `68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5` |
| `NotoEmoji-Regular.ttf` | <https://github.com/googlefonts/noto-emoji> | (added in font-bundle-redesign Phase 2) |
| `Inconsolata-Regular.ttf` | <https://github.com/googlefonts/Inconsolata> | (added in font-bundle-redesign Phase 2) |

The pinned SHA-256 values are the verification anchor in
`scripts/fetch-fonts.sh`. Mismatched bytes abort the fetch.

## Offline behavior

After a successful fetch, subsequent builds (including `cargo build
--release`) need no network access. Re-running `fetch-fonts.sh` while
offline succeeds when every declared file is already present and
SHA-256-matched — the script skips every up-to-date file before
attempting any network call.

## URL placeholders

When the upstream URL for an entry in `scripts/fetch-fonts.sh` is still
the literal string `TODO(URL)`, the script remains usable on machines
that already have a matching local copy (the SHA-256 short-circuit keeps
the no-op path alive). Downloading anew will fail with a clear error
until the URL is filled in.

## User overrides

Bundled fonts can be overridden without rebuilding emterm. Drop a
`.ttf` or `.otf` file into your platform's user font directory and
restart emterm:

- Linux: `~/.local/share/net.laser5.app.emterm/fonts/`
  (or `$XDG_DATA_HOME/net.laser5.app.emterm/fonts/` when set)
- Windows: `%APPDATA%\net.laser5.app.emterm\fonts\`

The resolver consults this directory ahead of system fonts and the
bundled fallback, so an override wins automatically.

## Updating

When refreshing a bundled font:

1. Update the SHA-256 column above.
2. Update the matching `fetch_one` call in `scripts/fetch-fonts.sh`
   with the new SHA-256 (and URL if the tag changes).
3. Delete the local file and re-run `make fetch-fonts` to verify the
   pinned hash matches.

## Platforms

Segoe UI Emoji is *not* bundled. On Windows, the renderer probes the
system fontdb for `Segoe UI Emoji` as a secondary emoji fallback.
