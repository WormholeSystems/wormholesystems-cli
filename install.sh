#!/bin/sh
# One-line installer for the Wormhole Systems setup wizard:
#   curl --proto '=https' --tlsv1.2 -sSf https://install.wormhole.systems | sh
# Pin a version with WSCTL_VERSION=v0.1.0.
set -eu

REPO="WormholeSystems/wormholesystems-cli"
VERSION="${WSCTL_VERSION:-latest}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  TARGET=x86_64-unknown-linux-gnu ;;
  Linux-aarch64) TARGET=aarch64-unknown-linux-gnu ;;
  Darwin-x86_64) TARGET=x86_64-apple-darwin ;;
  Darwin-arm64)  TARGET=aarch64-apple-darwin ;;
  *) echo "Unsupported platform: $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

if [ "$VERSION" = latest ]; then
  URL="https://github.com/$REPO/releases/latest/download/wsctl-$TARGET"
else
  URL="https://github.com/$REPO/releases/download/$VERSION/wsctl-$TARGET"
fi

if [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
  BIN_DIR=/usr/local/bin
else
  BIN_DIR="$HOME/.local/bin"
  mkdir -p "$BIN_DIR"
fi

echo "Downloading $URL"
curl -fsSL -o "$BIN_DIR/wsctl" "$URL"
chmod +x "$BIN_DIR/wsctl"
echo "Installed $("$BIN_DIR/wsctl" --version) to $BIN_DIR/wsctl"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Note: $BIN_DIR is not on your PATH." ;;
esac

# stdin is the curl pipe, so the interactive wizard needs the real terminal.
if [ -r /dev/tty ]; then
  printf "Run the setup wizard now? [Y/n] "
  read -r answer < /dev/tty 2>/dev/null || answer=n
  case "$answer" in
    [nN]*) echo "Run \`wsctl setup\` whenever you are ready." ;;
    *) exec "$BIN_DIR/wsctl" setup < /dev/tty ;;
  esac
fi
