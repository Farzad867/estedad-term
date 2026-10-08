#!/usr/bin/env bash
# Estedad Term installation script
set -euo pipefail

REPO="farzad/estedad-term"
INSTALL_DIR="${HOME}/.local/bin"
DESKTOP_DIR="${HOME}/.local/share/applications"

ARCH=$(uname -m)
if [ "$ARCH" != "x86_64" ]; then
    echo "Error: unsupported architecture '$ARCH'. Only x86_64 is supported." >&2
    exit 1
fi

mkdir -p "$INSTALL_DIR"
mkdir -p "$DESKTOP_DIR"

if [ -f "./target/release/estedad-term" ]; then
    echo "Installing local release binary..."
    cp -f "./target/release/estedad-term" "$INSTALL_DIR/"
    if [ -f "./assets/estedad-term.desktop" ]; then
        cp -f "./assets/estedad-term.desktop" "$DESKTOP_DIR/"
    fi
elif command -v cargo >/dev/null 2>&1 && [ -f "./Cargo.toml" ]; then
    echo "Building release binary via Cargo..."
    cargo build --release
    cp -f "./target/release/estedad-term" "$INSTALL_DIR/"
    if [ -f "./assets/estedad-term.desktop" ]; then
        cp -f "./assets/estedad-term.desktop" "$DESKTOP_DIR/"
    fi
else
    echo "Fetching release information from GitHub..."
    LATEST_TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep -Po '"tag_name": "\K.*?(?=")' || echo "v0.1.0")
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/estedad-term-linux-x86_64.tar.gz"

    TMP_DIR=$(mktemp -d)
    echo "Downloading ${DOWNLOAD_URL}..."
    if curl -fSL "$DOWNLOAD_URL" -o "${TMP_DIR}/estedad-term.tar.gz"; then
        tar -xzf "${TMP_DIR}/estedad-term.tar.gz" -C "$TMP_DIR"
        cp -f "${TMP_DIR}/estedad-term" "$INSTALL_DIR/"
        if [ -f "${TMP_DIR}/assets/estedad-term.desktop" ]; then
            cp -f "${TMP_DIR}/assets/estedad-term.desktop" "$DESKTOP_DIR/"
        fi
        rm -rf "$TMP_DIR"
    else
        echo "Error: failed to download release binary." >&2
        rm -rf "$TMP_DIR"
        exit 1
    fi
fi

chmod +x "$INSTALL_DIR/estedad-term"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

echo "Installation complete."
echo "Binary:  $INSTALL_DIR/estedad-term"
echo "Desktop: $DESKTOP_DIR/estedad-term.desktop"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        echo "Note: $INSTALL_DIR is not in PATH. Add the following to your shell profile:"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        ;;
esac
