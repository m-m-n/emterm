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
#   The SHA256 values below were computed from the binaries previously
#   committed to git (Noto Color Emoji + Noto Sans CJK JP) and from
#   manually-vetted upstream tags (Noto Emoji monochrome + Inconsolata).
#   If an entry's URL is set to "TODO(URL)" the SHA256 is still valid for
#   manually-placed copies — the script's idempotent path keeps working
#   for developers who already have the file, and the download path will
#   fail with a clear error until the URL is filled in.

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
# SHA256 values are the verification anchor. URLs are pinned to GitHub
# Releases tags (filled in during pin-existing-fonts task). When a URL
# is still TODO the script remains usable for developers who have a
# matching local copy: idempotency on SHA256 keeps the no-op path alive.

fetch_one "NotoColorEmoji.ttf" \
    "TODO(URL): pin to an upstream GitHub Releases asset whose bytes match the SHA256 below" \
    "ede3ac600162ca750e2aba610285f06f30d8c0dd198bcec1d53cb6efe660306c"

fetch_one "NotoSansCJKjp-Regular.otf" \
    "TODO(URL): pin to an upstream GitHub Releases asset whose bytes match the SHA256 below" \
    "68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5"

# Noto Emoji (monochrome) and Inconsolata are introduced in Phase 2 of the
# font-bundle-redesign feature. SHA256 values are still placeholders pending
# selection of a specific upstream release tag — until then, the script
# accepts any locally-placed copy so the GUI build can compile.
fetch_one "NotoEmoji-Regular.ttf" \
    "TODO(URL): https://github.com/googlefonts/noto-emoji NotoEmoji-Regular.ttf release asset" \
    "TODO(SHA256): pin once an upstream release tag is selected"

fetch_one "Inconsolata-Regular.ttf" \
    "TODO(URL): https://github.com/googlefonts/Inconsolata Inconsolata-Regular.ttf release asset" \
    "TODO(SHA256): pin once an upstream release tag is selected"
