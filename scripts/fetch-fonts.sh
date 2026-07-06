#!/usr/bin/env bash
# Fetch bundled fonts referenced by include_bytes! into src-tauri/assets/fonts/.
#
# - Idempotent: skips files whose SHA256 already matches the pinned value.
# - HTTPS only. No --insecure / -k.
# - Atomically moves the temp file into place after SHA256 verification.
# - Exits non-zero on download failure, hash mismatch, or missing tooling.
#
# Designed for both developer machines (offline after first fetch) and CI
# runners (`actions/cache` keyed on this script's hash short-circuits on hit).
#
# URL pinning notes:
#   URLs are pinned to immutable tagged refs on the upstream repos and
#   SHA256 values are taken from the bytes those refs serve. Updating a
#   font means bumping both the URL ref and the SHA256.

set -euo pipefail

DEST_DIR="${DEST_DIR:-src-tauri/assets/fonts}"
mkdir -p "$DEST_DIR"

# ---------------------------------------------------------------
# Tool detection (curl preferred, wget fallback)
# ---------------------------------------------------------------
DOWNLOAD_TOOL=""
if command -v curl >/dev/null 2>&1; then
    DOWNLOAD_TOOL="curl"
elif command -v wget >/dev/null 2>&1; then
    DOWNLOAD_TOOL="wget"
fi

if ! command -v sha256sum >/dev/null 2>&1; then
    printf 'fetch_fonts.tool_missing: sha256sum not found in PATH\n' >&2
    exit 2
fi

# ---------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------
sha_of() {
    sha256sum "$1" | awk '{print $1}'
}

download() {
    local url="$1" dest="$2"
    case "$url" in
        TODO\(URL\)*)
            printf 'fetch_fonts.url_missing: URL placeholder still set for %s\n' "$dest" >&2
            printf '  Edit scripts/fetch-fonts.sh to provide the upstream HTTPS URL.\n' >&2
            return 1
            ;;
        https://*) ;;
        *)
            printf 'fetch_fonts.insecure_url: refusing non-HTTPS URL %s\n' "$url" >&2
            return 1
            ;;
    esac

    case "$DOWNLOAD_TOOL" in
        curl)
            curl --fail --silent --show-error --location -o "$dest" "$url"
            ;;
        wget)
            wget --quiet -O "$dest" "$url"
            ;;
        *)
            printf 'fetch_fonts.tool_missing: neither curl nor wget found in PATH\n' >&2
            return 1
            ;;
    esac
}

fetch_one() {
    local name="$1" url="$2" expected_sha="$3"
    local dest="$DEST_DIR/$name"

    # When the SHA256 is still a placeholder, do not attempt to download.
    # The script remains usable for developers who already have a matching
    # local copy: any present file is accepted (with a clear warning) so
    # the GUI build can proceed while the upstream tag is being selected.
    case "$expected_sha" in
        TODO\(SHA256\)*)
            if [ -f "$dest" ]; then
                printf 'fetch-fonts: %s present (SHA256 not yet pinned — accepting local copy)\n' "$name"
                return 0
            fi
            printf 'fetch_fonts.sha_missing: %s SHA256 placeholder is set but no local copy exists\n' "$name" >&2
            printf '  Either place the file manually under %s/ or pin the SHA256 in scripts/fetch-fonts.sh.\n' "$DEST_DIR" >&2
            exit 1
            ;;
    esac

    if [ -f "$dest" ]; then
        local actual
        actual=$(sha_of "$dest")
        if [ "$actual" = "$expected_sha" ]; then
            printf 'fetch-fonts: %s up-to-date\n' "$name"
            return 0
        fi
        printf 'fetch-fonts: %s present but SHA256 mismatch (have=%s, want=%s) — re-downloading\n' \
            "$name" "$actual" "$expected_sha"
    fi

    printf 'fetch-fonts: downloading %s\n' "$name"
    local tmp
    tmp=$(mktemp "$DEST_DIR/.$name.XXXXXX")
    # Ensure tmp is cleaned up on early exit (preserved only on successful mv).
    trap 'rm -f "$tmp"' EXIT
    if ! download "$url" "$tmp"; then
        rm -f "$tmp"
        trap - EXIT
        printf 'fetch_fonts.download_failed: %s (url=%s)\n' "$name" "$url" >&2
        exit 1
    fi

    local actual
    actual=$(sha_of "$tmp")
    if [ "$actual" != "$expected_sha" ]; then
        rm -f "$tmp"
        trap - EXIT
        printf 'fetch_fonts.sha_mismatch: %s (expected=%s, got=%s)\n' \
            "$name" "$expected_sha" "$actual" >&2
        exit 1
    fi

    mv "$tmp" "$dest"
    trap - EXIT
    printf 'fetch-fonts: %s ok\n' "$name"
}

# ---------------------------------------------------------------
# Pinned font sources
# ---------------------------------------------------------------
# SHA256 values are the verification anchor. URLs are pinned to tagged
# refs on the upstream Google Fonts repositories (raw.githubusercontent
# .com), which are immutable per tag.

fetch_one "Noto-COLRv1.ttf" \
    "https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/Noto-COLRv1.ttf" \
    "0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2"

fetch_one "NotoSansCJKjp-Regular.otf" \
    "https://raw.githubusercontent.com/googlefonts/noto-cjk/Sans2.004/Sans/OTF/Japanese/NotoSansCJKjp-Regular.otf" \
    "68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5"

fetch_one "NotoSansCJKjp-Bold.otf" \
    "https://raw.githubusercontent.com/googlefonts/noto-cjk/Sans2.004/Sans/OTF/Japanese/NotoSansCJKjp-Bold.otf" \
    "e53dcb0dcb2922e45d01aae1ebd2f382bb81d4229b18b6b883bd170678af1f76"

# Modern monochrome Noto Emoji (v3.002, variable wght 300..700, default
# 400 = Regular). Distributed via the google/fonts repo, which has no
# release tags — the URL pins the commit that last touched the file,
# which is equally immutable. The legacy fonts/NotoEmoji-Regular.ttf in
# googlefonts/noto-emoji (v1.05, 888 glyphs) is missing text-default
# emoji like U+2602 ☂ / U+2603 ☃ / U+2639 ☹ / U+262F ☯, which made the
# fallback chain render them with the color emoji font.
fetch_one "NotoEmoji-Regular.ttf" \
    "https://raw.githubusercontent.com/google/fonts/b979dba422e445492b0eb9951ac52ee0b4d648c3/ofl/notoemoji/NotoEmoji%5Bwght%5D.ttf" \
    "de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551"

fetch_one "Inconsolata-Regular.otf" \
    "https://raw.githubusercontent.com/googlefonts/Inconsolata/v3.000/fonts/otf/Inconsolata-Regular.otf" \
    "5273275eab5b74743c799b8bda94644b10cfa861ef86badcd37d91cf9eac8a1c"

fetch_one "Inconsolata-Bold.otf" \
    "https://raw.githubusercontent.com/googlefonts/Inconsolata/v3.000/fonts/otf/Inconsolata-Bold.otf" \
    "4981b5ed672b90221525976bdebd53f66d6b758857aad021245acbae8e2efb9b"

fetch_one "NotoSansSymbols2-Regular.ttf" \
    "https://raw.githubusercontent.com/notofonts/noto-fonts/v20201206-phase3/hinted/ttf/NotoSansSymbols2/NotoSansSymbols2-Regular.ttf" \
    "e672040e4eb73c0d9ac841411eea34be3c1e12e81e88962786489fffae4fe973"
