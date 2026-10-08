# ترمینال استعداد (Estedad Term)

شبیه‌ساز ترمینال شتاب‌یافته گرافیکی (Vulkan) مبتنی بر زبان Rust، با پشتیبانی بومی از خط فارسی، متن دوجهته (BiDi) و تصحیح هوشمند خطاهای چیدمان صفحه‌کلید.

---

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Vulkan](https://img.shields.io/badge/Vulkan-wgpu%2024-red?logo=vulkan)](https://wgpu.rs/)
[![License: GPL-2.0](https://img.shields.io/badge/License-GPL--2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Wayland%20%7C%20X11-purple)](https://wayland.freedesktop.org/)

---

## قابلیت‌های اصلی

- **رندرینگ سخت‌افزاری با Vulkan:** پیاده‌سازی شده با پایپ‌لاین گرافیکی WGSL بر بستر کتابخانه `wgpu 24` و اطلس تکسچر با کارایی بالا و حداقل مصرف منابع سیستم.
- **شکل‌دهی کامل خط فارسی و متن دوجهته (BiDi):** اتصال استاندارد حروف فارسی و رندرینگ متون راست‌به‌چپ بدون به‌هم‌ریختگی با موتور `cosmic-text` بر پایه فونت فارسی **استعداد (Estedad)** و فونت نمادهای برنامه‌نویسی **CaskaydiaCove Nerd Font Mono**.
- **مفسر خودکار خطاهای چیدمان صفحه‌کلید و فینگلیش:**
  - تبدیل خودکار عبارات تایپ‌شده با چیدمان فارسی به معادل انگلیسی (مانند `سعیخ دشدخ` به `sudo nano` یا `لهف سفشفعس` به `git status`).
  - تبدیل دستورات آوانگاری‌شده و فینگلیش (مانند `سودو نانو` به `sudo nano` یا `کلیر` به `clear`).
  - پشتیبانی از عملگرهای ترکیبی خط فرمان (`&&`, `||`, `;`, `|`).
  - حفظ کامل کلمات، مسیرها و آرگومان‌های فارسی (مانند حفظ `پروژه` در دستور `mkdir پروژه`).
- **ظاهر و رنگ‌بندی:** طراحی شده بر پایه پالت رنگی Catppuccin Mocha با پشتیبانی از شفافیت پنجره در کامپوزیتورهای Wayland و سرورهای X11.

---

## پیش‌نیازها

- درایور Vulkan متناسب با کارت گرافیک (`vulkan-intel` یا `nvidia-utils` یا `vulkan-radeon`)
- فونت‌های سیستم: `Estedad` و `CaskaydiaCove Nerd Font Mono`
- کتابخانه‌های گرافیکی: `libwayland-client` و `libxkbcommon`

در توزیع‌های مبتنی بر آرچ (Arch / CachyOS / Manjaro):
```bash
sudo pacman -S --needed vulkan-intel ttf-caskaydia-cove-nerd
```

---

## روش‌های نصب

### روش اول: اسکریپت نصب سریع (پیشنهادی)

```bash
curl -fsSL https://raw.githubusercontent.com/Farzad867/estedad-term/master/install.sh | bash
```

### روش دوم: نصب مستقیم با Cargo از گیت‌هاب

```bash
cargo install --git https://github.com/Farzad867/estedad-term.git
```

### روش سوم: ساخت بسته محلی در آرچ لینوکس (PKGBUILD)

```bash
git clone https://github.com/Farzad867/estedad-term.git
cd estedad-term/packaging/aur
makepkg -si
```

### روش چهارم: کامپایل دستی از سورس‌کد

```bash
git clone https://github.com/Farzad867/estedad-term.git
cd estedad-term
cargo build --release
cp target/release/estedad-term ~/.local/bin/
cp assets/estedad-term.desktop ~/.local/share/applications/
```
