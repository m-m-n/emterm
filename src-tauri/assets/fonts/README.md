# Bundled Fonts (native-poc)

These fonts are bundled with `native-poc` and embedded via `include_bytes!` at
build time. Both are distributed under the SIL Open Font License 1.1; see
`LICENSE` in this directory.

## Inventory

| File | Upstream | Version | SHA-256 |
|---|---|---|---|
| `NotoColorEmoji.ttf` | <https://github.com/googlefonts/noto-emoji> | as-of 2025-06-23 | `ede3ac600162ca750e2aba610285f06f30d8c0dd198bcec1d53cb6efe660306c` |
| `NotoSansCJKjp-Regular.otf` | <https://github.com/googlefonts/noto-cjk> | Sans OTF JP Regular (2022-09-13) | `68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5` |

## Updating

When refreshing a bundled font, update both the file and the SHA-256 column in
this README. The SHA-256 row is the verification anchor used by the SDD
plan (see `doc/tasks/font-swash-migration/SPEC.md` §Security Considerations).

## Platforms

Segoe UI Emoji is *not* bundled. On Windows, the renderer probes the system
fontdb for `Segoe UI Emoji` as a secondary emoji fallback (per FR11).
