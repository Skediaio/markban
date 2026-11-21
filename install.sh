#!/bin/sh
# Installs the markban CLI and its agent skill.
#   curl -fsSL https://raw.githubusercontent.com/Skediaio/markban/main/install.sh | sh
# MARKBAN_INSTALL_DIR overrides the binary directory (default: ~/.local/bin).
set -eu

REPO_URL="https://github.com/Skediaio/markban/releases/latest/download"
INSTALL_DIR="${MARKBAN_INSTALL_DIR:-$HOME/.local/bin}"

fail() {
    echo "markban install: $*" >&2
    exit 1
}

os=$(uname -s)
arch=$(uname -m)
case "$os" in
    Darwin)
        case "$arch" in
            arm64 | aarch64) target=aarch64-apple-darwin ;;
            x86_64) target=x86_64-apple-darwin ;;
            *) fail "unsupported architecture '$arch' on macOS" ;;
        esac
        ;;
    Linux)
        case "$arch" in
            x86_64 | amd64) target=x86_64-unknown-linux-musl ;;
            arm64 | aarch64) fail "Linux aarch64 is not supported yet; build from source: cargo install --git https://github.com/Skediaio/markban markban-cli --locked" ;;
            *) fail "unsupported architecture '$arch' on Linux" ;;
        esac
        ;;
    *) fail "unsupported OS '$os'; on Windows use install.ps1" ;;
esac

download() {
    # download <url> <destination>
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2" || fail "download failed: $1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2" || fail "download failed: $1"
    else
        fail "neither curl nor wget found"
    fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "Downloading markban-$target"
download "$REPO_URL/markban-$target.tar.gz" "$tmp/markban.tar.gz"
tar -xzf "$tmp/markban.tar.gz" -C "$tmp" markban || fail "cannot extract the archive"
mkdir -p "$INSTALL_DIR"
cp "$tmp/markban" "$INSTALL_DIR/markban"
chmod +x "$INSTALL_DIR/markban"
if [ "$os" = "Darwin" ]; then
    xattr -d com.apple.quarantine "$INSTALL_DIR/markban" 2>/dev/null || true
fi

download "$REPO_URL/SKILL.md" "$tmp/SKILL.md"
for dir in "$HOME/.claude/skills/markban" "$HOME/.config/opencode/skills/markban"; do
    mkdir -p "$dir"
    cp "$tmp/SKILL.md" "$dir/SKILL.md"
done

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "Warning: $INSTALL_DIR is not on your PATH. Add this line to your shell profile:" >&2
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\"" >&2
        ;;
esac

echo "Installed: $("$INSTALL_DIR/markban" --version)"
echo "Skill installed in ~/.claude/skills/markban and ~/.config/opencode/skills/markban"
