#!/usr/bin/env bash
# Estedad Term installer
set -euo pipefail

REPO="Farzad867/estedad-term"
INSTALL_DIR="${HOME}/.local/bin"
DESKTOP_DIR="${HOME}/.local/share/applications"

info() {
    printf "[INFO] %s\n" "$*"
}

error() {
    printf "[ERROR] %s\n" "$*" >&2
}

ARCH=$(uname -m)
if [ "$ARCH" != "x86_64" ]; then
    error "Unsupported architecture '$ARCH'. Only x86_64 is supported."
    exit 1
fi

mkdir -p "$INSTALL_DIR"
mkdir -p "$DESKTOP_DIR"

if [ -f "./target/release/estedad-term" ]; then
    info "Installing local release binary..."
    cp -f "./target/release/estedad-term" "$INSTALL_DIR/"
    if [ -f "./assets/estedad-term.desktop" ]; then
        cp -f "./assets/estedad-term.desktop" "$DESKTOP_DIR/"
    fi
elif command -v cargo >/dev/null 2>&1 && [ -f "./Cargo.toml" ]; then
    info "Building release binary with Cargo..."
    cargo build --release
    cp -f "./target/release/estedad-term" "$INSTALL_DIR/"
    if [ -f "./assets/estedad-term.desktop" ]; then
        cp -f "./assets/estedad-term.desktop" "$DESKTOP_DIR/"
    fi
else
    info "Fetching latest release metadata from GitHub..."
    LATEST_TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep -Po '"tag_name": "\K.*?(?=")' || echo "v0.1.0")
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/estedad-term-linux-x86_64.tar.gz"

    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    info "Downloading ${DOWNLOAD_URL}..."
    if curl -fSL "$DOWNLOAD_URL" -o "${TMP_DIR}/estedad-term.tar.gz"; then
        tar -xzf "${TMP_DIR}/estedad-term.tar.gz" -C "$TMP_DIR"
        cp -f "${TMP_DIR}/estedad-term" "$INSTALL_DIR/"
        if [ -f "${TMP_DIR}/assets/estedad-term.desktop" ]; then
            cp -f "${TMP_DIR}/assets/estedad-term.desktop" "$DESKTOP_DIR/"
        fi
    else
        error "Failed to download release archive."
        exit 1
    fi
fi

chmod +x "$INSTALL_DIR/estedad-term"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

info "Installation completed successfully."
info "Binary:  $INSTALL_DIR/estedad-term"
info "Desktop: $DESKTOP_DIR/estedad-term.desktop"

if ! command -v estedad-term >/dev/null 2>&1; then
    printf "\nNotice: %s is not currently in your PATH.\n" "$INSTALL_DIR"
    printf "Consider adding it to your shell configuration:\n"
    printf "  export PATH=\"\$HOME/.local/bin:\$PATH\"\n\n"
fi
