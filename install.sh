#!/usr/bin/env bash
# اسکریپت نصب استعداد ترم (Estedad Term)
set -euo pipefail

REPO="Farzad867/estedad-term"
INSTALL_DIR="${HOME}/.local/bin"
DESKTOP_DIR="${HOME}/.local/share/applications"

ARCH=$(uname -m)
if [ "$ARCH" != "x86_64" ]; then
    echo "خطا: معماری پردازنده ($ARCH) پشتیبانی نمی‌شود. تنها نسخه x86_64 پشتیبانی می‌شود." >&2
    exit 1
fi

mkdir -p "$INSTALL_DIR"
mkdir -p "$DESKTOP_DIR"

if [ -f "./target/release/estedad-term" ]; then
    echo "در حال نصب فایل باینری کامپایل‌شده محلی..."
    cp -f "./target/release/estedad-term" "$INSTALL_DIR/"
    if [ -f "./assets/estedad-term.desktop" ]; then
        cp -f "./assets/estedad-term.desktop" "$DESKTOP_DIR/"
    fi
elif command -v cargo >/dev/null 2>&1 && [ -f "./Cargo.toml" ]; then
    echo "در حال کامپایل پروژه با ابزار Cargo..."
    cargo build --release
    cp -f "./target/release/estedad-term" "$INSTALL_DIR/"
    if [ -f "./assets/estedad-term.desktop" ]; then
        cp -f "./assets/estedad-term.desktop" "$DESKTOP_DIR/"
    fi
else
    echo "در حال دریافت اطلاعات نسخه رسمی از مخزن گیت‌هاب..."
    LATEST_TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep -Po '"tag_name": "\K.*?(?=")' || echo "v0.1.0")
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/estedad-term-linux-x86_64.tar.gz"

    TMP_DIR=$(mktemp -d)
    echo "در حال دانلود بسته باینری از: ${DOWNLOAD_URL}..."
    if curl -fSL "$DOWNLOAD_URL" -o "${TMP_DIR}/estedad-term.tar.gz"; then
        tar -xzf "${TMP_DIR}/estedad-term.tar.gz" -C "$TMP_DIR"
        cp -f "${TMP_DIR}/estedad-term" "$INSTALL_DIR/"
        if [ -f "${TMP_DIR}/assets/estedad-term.desktop" ]; then
            cp -f "${TMP_DIR}/assets/estedad-term.desktop" "$DESKTOP_DIR/"
        fi
        rm -rf "$TMP_DIR"
    else
        echo "خطا: دانلود نسخه باینری ناموفق بود." >&2
        rm -rf "$TMP_DIR"
        exit 1
    fi
fi

chmod +x "$INSTALL_DIR/estedad-term"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

echo "عملیات نصب با موفقیت پایان یافت."
echo "مسیر فایل اجرایی: $INSTALL_DIR/estedad-term"
echo "میانبر دسکتاپ:    $DESKTOP_DIR/estedad-term.desktop"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        echo "توجه: مسیر $INSTALL_DIR در متغیر PATH تعریف نشده است. خط زیر را به تنظیمات شل خود اضافه نمایید:"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        ;;
esac
