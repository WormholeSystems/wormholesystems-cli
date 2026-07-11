#!/bin/sh
# Bootstrap for the Wormhole Systems setup wizard.
#
# This file is meant to be committed to the wormholesystems-containers
# repo (as ./setup.sh) together with a .wsctl-version file pinning the
# wizard release that repo revision was tested against. It downloads the
# matching prebuilt binary for the current platform and runs it — no
# Rust toolchain needed on the server.
set -eu

REPO="WormholeSystems/wormholesystems-cli"
DIR="$(cd "$(dirname "$0")" && pwd)"
VERSION="$(cat "$DIR/.wsctl-version" 2>/dev/null || echo latest)"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)   TARGET=x86_64-unknown-linux-gnu ;;
  Linux-aarch64)  TARGET=aarch64-unknown-linux-gnu ;;
  Darwin-x86_64)  TARGET=x86_64-apple-darwin ;;
  Darwin-arm64)   TARGET=aarch64-apple-darwin ;;
  *) echo "Unsupported platform: $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

if [ "$VERSION" = latest ]; then
  URL="https://github.com/$REPO/releases/latest/download/wsctl-$TARGET"
else
  URL="https://github.com/$REPO/releases/download/$VERSION/wsctl-$TARGET"
fi

BIN="$DIR/.wsctl"
echo "Downloading $URL"
curl -fsSL -o "$BIN" "$URL"
chmod +x "$BIN"
exec "$BIN" setup --dir "$DIR" "$@"
