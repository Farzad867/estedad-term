# Estedad Term (استعداد ترم) 🚀

> **ترمینال شتاب‌یافته گرافیکی (Vulkan) با پشتیبانی بومی از زبان فارسی، تصحیح هوشمند دستورات و تم مدرن Catppuccin**
>
> A blazing-fast, GPU-accelerated terminal emulator built in Rust with native Persian/Arabic BiDi shaping, keyboard typo correction, and Vulkan rendering.

---

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Vulkan](https://img.shields.io/badge/Vulkan-wgpu%2024-red?logo=vulkan)](https://wgpu.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Wayland%20%7C%20X11-purple)](https://wayland.freedesktop.org/)

---

## ✨ ویژگی‌های برجسته (Key Features)

### ⚡ رندرینگ سخت‌افزاری فوق‌العاده سریع با Vulkan
- پیاده‌سازی شده با پایپ‌لاین گرافیکی مدرن **WGSL** بر بستر **`wgpu 24`**.
- اطلس تکسچر با ابعاد ۲۰۴۸x۲۰۴۸ برای ذخیره و رندر بدون لگ فونت‌ها با یک تک‌درخواست رسم (Single Instanced Draw Call per frame).
- مصرف پردازنده نزدیک به صفر (0.0% CPU) در حالت بی‌کار (Idle) به لطف هماهنگی با رویدادهای Wayland/X11.

### 🇮🇷 پشتیبانی اصیل از خط فارسی و چینش راست‌به‌چپ (BiDi)
- حل کامل معضل حروف بریده‌بریده و معکوس در ترمینال‌های رایج لینوکس با استفاده از موتور متن **`cosmic-text`**.
- ادغام دو فونت استاندارد: فونت فارسی زیبای **Estedad** به همراه **CaskaydiaCove Nerd Font Mono** برای آیکون‌ها و نمادهای برنامه‌نویسی.

### 🧠 مفسر هوشمند دستورات و تبدیل خطاهای تایپی (Typo Corrector)
- **تبدیل خطای چیدمان کیبورد:** اگر کیبورد روی فارسی بود و انگلیسی تایپ کردید، خودکار اصلاح می‌شود:
  - `سعیخ دشدخ` ⬅️ `sudo nano`
  - `لهف سفشفعس` ⬅️ `git status`
  - `مس -مش` ⬅️ `ls -la`
- **پشتیبانی از دستورات فینگلیش:**
  - `سودو نانو` ⬅️ `sudo nano`
  - `کلیر` ⬅️ `clear`
- **حفظ آرگومان‌ها و مقادیر فارسی:** نام پوشه‌ها یا متن‌های فارسی تغییر نمی‌کنند:
  - `mkdir سلام` ⬅️ پوشه‌ای با نام «سلام» ساخته می‌شود.
- **زنجیره‌سازی دستورات ترکیبی:** پشتیبانی کامل از عملگرهای `&&`, `||`, `;`, و `|`.

### 🎨 تم تیره Catppuccin Mocha و شفافیت شیشه‌ای
- رنگ پس‌زمینه چشم‌نواز `#1E1E2E` هماهنگ با اکوسیستم مدرن لینوکس.
- شفافیت واقعی پنجره با حالت Pre-multiplied Alpha متناسب با کامپوزیتورهای Wayland (مانند Sway و Hyprland).

### 🔋 مدیریت هوشمند پردازنده گرافیکی (Dual-GPU)
- به صورت خودکار از گرافیک کم‌مصرف (Intel iGPU) استفاده می‌کند تا شارژ باتری لپ‌تاپ بهینه بماند.
- امکان سوییچ دستی به کارت انویدیا تنها با یک متغیر محیطی:
  ```bash
  ESTEDAD_GPU=nvidia estedad-term
  ```

### 🖱️ بدون فریز در برنامه‌های تعاملی (btop, htop)
- هندلینگ بی‌درنگ ماوس و رویدادهای PTY.
- پشتیبانی کامل از انتخاب کلمه (دابل‌کلیک)، خط (تریپل‌کلیک) و انتخاب محدوده بدون هیچ‌گونه فریز یا بن‌بست پردازشی (Deadlock-Free).

---

## 📦 پیش‌نیازها (Prerequisites)

برای اجرای روان و بهترین تجربه، بسته‌های زیر را روی توزیع لینوکس خود داشته باشید:

- درایور Vulkan متناسب با کارت گرافیک (`vulkan-intel` یا `nvidia-utils` یا `vulkan-radeon`)
- فونت **Estedad** و فونت مونوگیفیک کدنویسی **CaskaydiaCove Nerd Font Mono**
- کتابخانه‌های Wayland / X11 (`libwayland-dev`, `libxkbcommon`)

در آرچ / CachyOS / مانجارو:
```bash
sudo pacman -S --needed vulkan-intel ttf-caskaydia-cove-nerd
```

---

## 🛠️ نحوه نصب و ساخت (Build & Installation)

### روش اول: کامپایل از روی سورس کد (Rust)

```bash
# ۱. دریافت ریپازیتوری
git clone https://github.com/farzad/estedad-term.git
cd estedad-term

# ۲. کامپایل نسخه بهینه (Release)
cargo build --release

# ۳. قرار دادن فایل اجرایی در مسیر سیستم
cp target/release/estedad-term ~/.local/bin/

# ۴. افزودن آیکون و میانبر به منوی برنامه‌ها
cp assets/estedad-term.desktop ~/.local/share/applications/
```

### روش دوم: نصب در توزیع‌های آرچ از طریق AUR

```bash
cd packaging/aur
makepkg -si
```

---

## ⚙️ تنظیم در مدیر پنجره‌ها (Window Managers)

### تنظیم در Sway (`~/.config/sway/config`):
```sway
bindsym $mod+Return exec ~/.local/bin/estedad-term
```

### تنظیم در i3 (`~/.config/i3/config`):
```i3
bindsym $mod+Return exec ~/.local/bin/estedad-term
```

### تنظیم در Hyprland (`~/.config/hypr/hyprland.conf`):
```conf
bind = $mainMod, Return, exec, estedad-term
```

---

## 🧪 اجرای آزمون‌ها (Running Tests)

برای اطمینان از صحت تمام بخش‌ها (رندرینگ Vulkan، ترنسلیتر دستورات، مفسر بای‌دی و رفع ددلاک‌ها):

```bash
cargo test
```

تمام ۲۱ آزمون یکپارچه بدون خطا پاس می‌شوند.

---

## 📜 لایسنس (License)

این پروژه تحت مجوز آزاد [MIT License](LICENSE) منتشر شده است. استفاده، ویرایش و بازنشر آن آزاد است.
