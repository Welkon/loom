#!/bin/sh
# Install the loom CLI from GitHub releases.
#
#   curl -fsSL https://raw.githubusercontent.com/Welkon/loom/main/install.sh | sh
#
# Environment:
#   LOOM_VERSION  release to install (default: latest)
#   LOOM_BINDIR   install directory (default: ~/.local/bin)
set -eu

repo="Welkon/loom"
version="${LOOM_VERSION:-latest}"
bindir="${LOOM_BINDIR:-$HOME/.local/bin}"

if [ "$version" = "latest" ]; then
  version="$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
fi
if [ -z "$version" ]; then
  echo "loom install: could not resolve the latest release" >&2
  exit 1
fi

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *)
    echo "loom install: unsupported system $(uname -s); download the binary from" >&2
    echo "  https://github.com/$repo/releases" >&2
    exit 1
    ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=arm64 ;;
  *)
    echo "loom install: unsupported machine $(uname -m)" >&2
    exit 1
    ;;
esac

asset="loom-$os-$arch"
base="https://github.com/$repo/releases/download/$version"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "loom install: $version ($os-$arch)"
curl -fsSL "$base/$asset" -o "$tmp/$asset"
curl -fsSL "$base/checksums-sha256.txt" -o "$tmp/checksums-sha256.txt"

expected="$(sed -n "s/^\([0-9a-f]*\)  $asset$/\1/p" "$tmp/checksums-sha256.txt")"
if [ -z "$expected" ]; then
  echo "loom install: $asset is not in checksums-sha256.txt" >&2
  exit 1
fi
actual="$(sha256sum "$tmp/$asset" 2>/dev/null | cut -d' ' -f1 || shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)"
if [ "$expected" != "$actual" ]; then
  echo "loom install: checksum mismatch for $asset" >&2
  echo "  expected $expected" >&2
  echo "  actual   $actual" >&2
  exit 1
fi

mkdir -p "$bindir"
install -m 0755 "$tmp/$asset" "$bindir/loom"
echo "loom install: $("$bindir/loom" --version) at $bindir/loom"
case ":$PATH:" in
  *":$bindir:"*) ;;
  *) echo "loom install: add $bindir to PATH" ;;
esac
