#!/bin/sh
set -eu

say() { printf '%s
' "$*"; }
die() { printf 'aw installer: %s
' "$*" >&2; exit 1; }

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "sha256sum or shasum is required for checksum verification"
  fi
}

VERSION="${AW_VERSION:-latest}"
REPO="${AW_REPO:-abird-ai/aw}"
BASE_URL="${AW_RELEASE_BASE_URL:-}"

if [ -n "${AW_INSTALL_DIR:-}" ]; then
  INSTALL_DIR="$AW_INSTALL_DIR"
elif [ -n "${HOME:-}" ]; then
  INSTALL_DIR="$HOME/.local/bin"
else
  die "HOME is not set; set AW_INSTALL_DIR explicitly"
fi

case "$(uname -s 2>/dev/null || printf unknown)" in
  Linux)
    case "$(uname -m 2>/dev/null || printf unknown)" in
      x86_64|amd64) ASSET="aw-linux-x86_64"; BINARY="aw" ;;
      aarch64|arm64) ASSET="aw-linux-aarch64"; BINARY="aw" ;;
      *) die "unsupported Linux architecture: $(uname -m). Published: x86_64, aarch64." ;;
    esac
    ;;
  MINGW*|MSYS*|CYGWIN*)
    case "$(uname -m 2>/dev/null || printf unknown)" in
      x86_64|amd64) ASSET="aw-windows-x86_64.exe"; BINARY="aw.exe" ;;
      aarch64|arm64) ASSET="aw-windows-aarch64.exe"; BINARY="aw.exe" ;;
      *) die "unsupported Windows architecture: $(uname -m). Published: x86_64, arm64." ;;
    esac
    ;;
  Darwin)
    case "$(uname -m 2>/dev/null || printf unknown)" in
      arm64|aarch64) ASSET="aw-macos-aarch64"; BINARY="aw" ;;
      *) die "unsupported macOS architecture: $(uname -m). Published: Apple Silicon (arm64)." ;;
    esac
    ;;
  *) die "unsupported OS: $(uname -s 2>/dev/null || printf unknown)" ;;
esac

if [ -z "$BASE_URL" ]; then
  if [ "$VERSION" = "latest" ]; then
    BASE_URL="https://github.com/${REPO}/releases/latest/download"
  else
    case "$VERSION" in
      v*) TAG="$VERSION" ;;
      *) TAG="v$VERSION" ;;
    esac
    BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"
  fi
fi

command -v curl >/dev/null 2>&1 || die "curl is required"

TMPDIR_ROOT="${TMPDIR:-/tmp}"
WORKDIR="$(mktemp -d "${TMPDIR_ROOT%/}/aw-install.XXXXXX")"
trap 'rm -rf "$WORKDIR"' EXIT HUP INT TERM

say "Downloading aw..."
curl -fsSL "${BASE_URL%/}/${ASSET}" -o "$WORKDIR/$ASSET"
curl -fsSL "${BASE_URL%/}/${ASSET}.sha256" -o "$WORKDIR/$ASSET.sha256"
curl -fsSL "${BASE_URL%/}/VERSION" -o "$WORKDIR/VERSION"

[ "$(wc -l < "$WORKDIR/$ASSET.sha256" | tr -d ' ')" -eq 1 ] || die "invalid SHA-256 sidecar"
[ "$(awk 'NR == 1 { print NF; exit }' "$WORKDIR/$ASSET.sha256")" -eq 2 ] || die "invalid SHA-256 sidecar"
expected="$(awk 'NR == 1 { print $1; exit }' "$WORKDIR/$ASSET.sha256" | tr 'A-F' 'a-f')"
named="$(awk 'NR == 1 { print $2; exit }' "$WORKDIR/$ASSET.sha256")"
named="${named#\*}"
[ "${#expected}" -eq 64 ] || die "invalid SHA-256 sidecar"
case "$expected" in *[!0-9a-f]*) die "invalid SHA-256 sidecar" ;; esac
[ "$named" = "$ASSET" ] || die "checksum sidecar names an unexpected asset"

actual="$(sha256_file "$WORKDIR/$ASSET" | tr 'A-F' 'a-f')"
[ "$expected" = "$actual" ] || die "SHA-256 verification failed"

new_version="$(tr -d '\r\n' < "$WORKDIR/VERSION")"
[ -n "$new_version" ] && [ "${#new_version}" -le 64 ] || die "invalid release VERSION metadata"
case "$new_version" in *[!0-9A-Za-z.+-]*) die "invalid release VERSION metadata" ;; esac
if [ "$VERSION" != "latest" ]; then
  case "$VERSION" in v*) requested_version="${VERSION#v}" ;; *) requested_version="$VERSION" ;; esac
  [ "$new_version" = "$requested_version" ] ||
    die "downloaded aw version $new_version does not match requested version $requested_version"
fi

mkdir -p "$INSTALL_DIR"
DESTINATION="$INSTALL_DIR/$BINARY"
had_existing=false
installed_hash=""

if [ -f "$DESTINATION" ]; then
  had_existing=true
  installed_hash="$(sha256_file "$DESTINATION" | tr 'A-F' 'a-f')"
fi

if [ "$installed_hash" = "$expected" ]; then
  say "aw $new_version is already up to date at $DESTINATION"
else
  DEST_TMP="$INSTALL_DIR/.$BINARY.tmp.$$"
  trap 'rm -rf "$WORKDIR"; [ -z "${DEST_TMP:-}" ] || rm -f "$DEST_TMP"' EXIT HUP INT TERM
  cp "$WORKDIR/$ASSET" "$DEST_TMP"
  chmod 0755 "$DEST_TMP"
  mv -f "$DEST_TMP" "$DESTINATION"
  DEST_TMP=""

  if [ "$had_existing" = true ]; then
    say "Updated aw to $new_version at $DESTINATION"
  else
    say "Installed aw $new_version to $DESTINATION"
  fi
fi

case ":${PATH:-}:" in
  *":$INSTALL_DIR:"*) ;;
  *) say "Add $INSTALL_DIR to PATH to run 'aw' directly." ;;
esac
