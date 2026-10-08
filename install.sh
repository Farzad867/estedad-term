#!/usr/bin/env bash
# Estedad Term (استعداد ترم) Installer Script
set -e

REPO="farzad/estedad-term"
INSTALL_DIR="${HOME}/.local/bin"
DESKTOP_DIR="${HOME}/.local/share/applications"

echo "=================================================="
echo " 🚀 نصب ترمینال استعداد (Estedad Term Installer) "
echo "=================================================="

# Check architecture
ARCH=$(uname -m)
if [ "$ARCH" != "x86_64" ]; then
    echo "⚠️ اخطار: سیستم شما $ARCH است. در حال حاضر نسخه باینری x86_64 پشتیبانی می‌شود."
    exit 1
fi

mkdir -p "$INSTALL_DIR"
mkdir -p "$DESKTOP_DIR"

# Check if script is executed from inside git repo or remotely
if [ -f "./target/release/estedad-term" ]; then
    echo "📦 در حال نصب نسخه کامپایل شده محلی..."
    cp -f "./target/release/estedad-term" "$INSTALL_DIR/"
    if [ -f "./assets/estedad-term.desktop" ]; then
        cp -f "./assets/estedad-term.desktop" "$DESKTOP_DIR/"
    fi
elif command -v cargo >/dev/null 2>&1 && [ -f "./Cargo.toml" ]; then
    echo "🔨 کامپایل پروژه با Cargo..."
    cargo build --release
    cp -f "./target/release/estedad-term" "$INSTALL_DIR/"
    if [ -f "./assets/estedad-term.desktop" ]; then
        cp -f "./assets/estedad-term.desktop" "$DESKTOP_DIR/"
    fi
else
    echo "🌐 دریافت آخرین نسخه انتشاریافته از گیت‌هاب..."
    LATEST_TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep -Po '"tag_name": "\K.*?(?=")' || echo "v0.1.0")
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/estedad-term-linux-x86_64.tar.gz"
    
    TMP_DIR=$(mktemp -d)
    echo "⬇️ دانلود از ${DOWNLOAD_URL}..."
    if curl -fSL "$DOWNLOAD_URL" -o "${TMP_DIR}/estedad-term.tar.gz"; then
        tar -xzf "${TMP_DIR}/estedad-term.tar.gz" -C "$TMP_DIR"
        cp -f "${TMP_DIR}/estedad-term" "$INSTALL_DIR/"
        if [ -f "${TMP_DIR}/assets/estedad-term.desktop" ]; then
            cp -f "${TMP_DIR}/assets/estedad-term.desktop" "$DESKTOP_DIR/"
        fi
        rm -rf "$TMP_DIR"
    else
        echo "❌ دانلود نسخه آماده ناموفق بود. در صورت داشتن Rust می‌توانید از روش 'cargo install --git' استفاده کنید."
        rm -rf "$TMP_DIR"
        exit 1
    fi
fi

chmod +x "$INSTALL_DIR/estedad-term"

# Refresh desktop database if available
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

echo ""
echo "✅ نصب با موفقیت انجام شد!"
echo "📍 مسیر فایل اجرایی: $INSTALL_DIR/estedad-term"
echo "🖥️ شورت‌کات دسکتاپ: $DESKTOP_DIR/estedad-term.desktop"
echo ""

# Check PATH
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "💡 نکته: مسیر ~/.local/bin در متغیر PATH شما نیست. خط زیر را به ~/.bashrc یا ~/.zshrc اضافه کنید:"
        echo 'export PATH="$HOME/.local/bin:$PATH"'
        ;;
esac

echo ""
echo "برای اجرا کافیست دستور زیر را وارد کنید:"
echo "estedad-term"
