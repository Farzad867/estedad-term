# ترمینال استعداد (Estedad Term)

شبیه‌ساز ترمینال شتاب‌یافته گرافیکی (Vulkan) مبتنی بر زبان Rust، با پشتیبانی بومی از خط فارسی، تصحیح خودکار خطاهای چیدمان صفحه‌کلید و شکل‌دهی متن دوجهته (BiDi).

---

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Vulkan](https://img.shields.io/badge/Vulkan-wgpu%2024-red?logo=vulkan)](https://wgpu.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Wayland%20%7C%20X11-purple)](https://wayland.freedesktop.org/)

---

## معرفی

استعداد ترم (Estedad Term) یک شبیه‌ساز ترمینال مدرن برای سیستم‌عامل‌های یونیکسی (گنو/لینوکس) است که با هدف حل مشکلات دیرینه نمایش زبان‌های راست‌به‌چپ (به‌ویژه خط و زبان فارسی) و ارتقای کارایی رندرینگ از طریق رابط‌های گرافیکی مدرن توسعه یافته است.

---

## قابلیت‌های فنی

- **رندرینگ سخت‌افزاری با Vulkan:** پیاده‌سازی شده با پایپ‌لاین گرافیکی و شیدرهای WGSL بر بستر کتابخانه `wgpu 24`. استفاده از اطلس تکسچر با ابعاد ۲۰۴۸ در ۲۰۴۸ پیکسل و اعمال ترسیم با یک فراخوانی در هر فریم (Single Instanced Draw Call) با حداقل مصرف پردازنده در حالت آماده‌باش.
- **شکل‌دهی بومی خط فارسی و متن دوجهته (BiDi):** اتصال صحیح حروف و مدیریت جهت متن بر پایه موتور `cosmic-text`. استفاده پیش‌فرض از خانواده فونت فارسی **استعداد (Estedad)** به همراه فونت کمکی **CaskaydiaCove Nerd Font Mono** جهت پوشش آیکون‌های خط فرمان و علائم برنامه‌نویسی.
- **مفسر خودکار خطاهای چیدمان صفحه‌کلید و فینگلیش:** تحلیل هوشمند ورودی پیش از ارسال به PTY:
  - خطاهای چیدمان صفحه‌کلید: تبدیل خودکار عبارات تایپ‌شده در وضعیت چیدمان فارسی به معادل انگلیسی (مانند تبدیل `سعیخ دشدخ` به `sudo nano`، `لهف سفشفعس` به `git status` و `مس -مش` به `ls -la`).
  - دستورات آوانگاری‌شده (فینگلیش): تبدیل عبارات رایج نظیر `سودو نانو` به `sudo nano` و `کلیر` به `clear`.
  - پشتیبانی از عملگرهای ترکیبی شل: پشتیبانی از عملگرهای منطقی و لوله‌کشی خط فرمان (`&&`, `||`, `;`, `|`).
  - حفظ مقادیر فارسی: عدم تغییر در آرگومان‌ها، اسامی پوشه‌ها و مسیرهای فارسی (مانند حفظ عبارت `سلام` در دستور `mkdir سلام`).
- **یکپارچگی با کامپوزیتورها و پالت رنگی Catppuccin:** پشتیبانی از شفافیت پنجره با حالت Pre-multiplied Alpha متناسب با کامپوزیتورهای Wayland (نظیر Sway و Hyprland) بر اساس پالت رنگی استاندارد Catppuccin Mocha.
- **مدیریت توان مصرفی و پردازنده‌های گرافیکی دوگانه:** انتخاب پیش‌فرض پردازنده گرافیکی کم‌مصرف (Intel iGPU) به منظور بهینه‌سازی مصرف باتری، با قابلیت سوئیچ به پردازنده گرافیکی مجزا (مانند NVIDIA) از طریق متغیر محیطی.
- **پایداری در برنامه‌های تعاملی تمام‌صفحه:** مدیریت بدون وقفه رویدادهای ماوس و PTY بدون بروز بن‌بست (Deadlock-Free) حین انتخاب متن، دوبار کلیک و سه‌بار کلیک در ابزارهایی نظیر `btop` و `htop`.

---

## پیش‌نیازها

پیش‌نیازهای اجرای برنامه:
- درایور Vulkan متناسب با سخت‌افزار (`vulkan-intel` یا `nvidia-utils` یا `vulkan-radeon`)
- فونت‌های `Estedad` و `CaskaydiaCove Nerd Font Mono`
- کتابخانه‌های رابط گرافیکی: `libwayland-client` و `libxkbcommon`

در توزیع‌های مبتنی بر آرچ (Arch / CachyOS / Manjaro):
```bash
sudo pacman -S --needed vulkan-intel ttf-caskaydia-cove-nerd
```

---

## روش‌های نصب

### روش اول: اسکریپت نصب خودکار

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

### روش چهارم: کامپایل دستی از روی سورس‌کد

```bash
git clone https://github.com/Farzad867/estedad-term.git
cd estedad-term
cargo build --release
cp target/release/estedad-term ~/.local/bin/
cp assets/estedad-term.desktop ~/.local/share/applications/
```

---

## پیکربندی مدیر پنجره‌ها

### مدیر پنجره Sway (`~/.config/sway/config`)

```sway
bindsym $mod+Return exec ~/.local/bin/estedad-term
```

### مدیر پنجره i3 (`~/.config/i3/config`)

```i3
bindsym $mod+Return exec ~/.local/bin/estedad-term
```

### مدیر پنجره Hyprland (`~/.config/hypr/hyprland.conf`)

```conf
bind = $mainMod, Return, exec, estedad-term
```

---

## متغیرهای محیطی

| متغیر | مقادیر معتبر | شرح عملکرد |
|---|---|---|
| `ESTEDAD_GPU` | `intel`, `nvidia`, `default` | تعیین پردازنده گرافیکی هدف (پیش‌فرض: پردازنده گرافیکی مجتمع) |
| `WGPU_BACKEND` | `vulkan`, `gl` | تعیین بستر گرافیکی رندرینگ (پیش‌فرض: Vulkan) |

---

## آزمون‌ها

اجرای آزمون‌های یکپارچگی و ارزیابی صحت عملکرد:

```bash
cargo test
```

---

## مجوز انتشار

این نرم‌افزار تحت مجوز آزاد [MIT License](LICENSE) منتشر شده است.
