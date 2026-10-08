# Estedad Term

A GPU-accelerated terminal emulator built with Rust and Vulkan, featuring native Persian/Arabic bidirectional (BiDi) text shaping and intelligent command translation.

---

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Vulkan](https://img.shields.io/badge/Vulkan-wgpu%2024-red?logo=vulkan)](https://wgpu.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Wayland%20%7C%20X11-purple)](https://wayland.freedesktop.org/)

---

## Overview

Estedad Term is designed for Unix systems, providing native Right-to-Left (RTL) and bidirectional (BiDi) text handling alongside high-performance hardware rendering via modern Vulkan pipelines.

### Key Capabilities

- **Hardware Acceleration:** Built on `wgpu 24` using custom WGSL shaders and a 2048x2048 glyph texture atlas. Executes single-instanced draw calls per frame with near-zero idle CPU usage.
- **Bidirectional Text & Shaping:** Native Persian and Arabic glyph shaping powered by `cosmic-text`. Configured with the **Estedad** font family and fallback support for **CaskaydiaCove Nerd Font Mono**.
- **Keyboard Layout & Typo Translation:** Automatically maps Persian layout keystrokes and transliterated commands to system commands before PTY submission:
  - Layout typos: `سعیخ دشدخ` maps to `sudo nano`, `لهف سفشفعس` maps to `git status`, `مس -مش` maps to `ls -la`.
  - Transliterated commands: `سودو نانو` maps to `sudo nano`, `کلیر` maps to `clear`.
  - Operator preservation: Supports pipelines and conditional operators (`&&`, `||`, `;`, `|`).
  - Argument preservation: Unicode arguments and paths (e.g., `mkdir سلام`) remain unchanged.
- **Compositor Integration:** Window transparency with pre-multiplied alpha designed for Wayland compositors (Sway, Hyprland) using the Catppuccin Mocha color scheme (`#1E1E2E`).
- **Power Management:** Automatically selects low-power integrated graphics (Intel iGPU) by default to preserve battery life, with environment variable override (`ESTEDAD_GPU=nvidia`) for dedicated GPUs.
- **Terminal Interactivity:** Non-blocking PTY handling with full mouse tracking and deadlock-free word/line selection in full-screen terminal applications (`btop`, `htop`).

---

## Dependencies

Runtime requirements:
- Vulkan driver (`vulkan-intel`, `nvidia-utils`, or `vulkan-radeon`)
- Fonts: `Estedad` and `CaskaydiaCove Nerd Font Mono`
- Wayland / X11 libraries: `libwayland-client`, `libxkbcommon`

On Arch Linux / CachyOS:
```bash
sudo pacman -S --needed vulkan-intel ttf-caskaydia-cove-nerd
```

---

## Installation

### Method 1: Automated Installer

```bash
curl -fsSL https://raw.githubusercontent.com/farzad/estedad-term/master/install.sh | bash
```

### Method 2: Cargo (Git)

```bash
cargo install --git https://github.com/farzad/estedad-term.git
```

### Method 3: Arch Linux / AUR (Local Build)

```bash
git clone https://github.com/farzad/estedad-term.git
cd estedad-term/packaging/aur
makepkg -si
```

### Method 4: Manual Build from Source

```bash
git clone https://github.com/farzad/estedad-term.git
cd estedad-term
cargo build --release
cp target/release/estedad-term ~/.local/bin/
cp assets/estedad-term.desktop ~/.local/share/applications/
```

---

## Configuration

### Sway (`~/.config/sway/config`)

```sway
bindsym $mod+Return exec ~/.local/bin/estedad-term
```

### i3 (`~/.config/i3/config`)

```i3
bindsym $mod+Return exec ~/.local/bin/estedad-term
```

### Hyprland (`~/.config/hypr/hyprland.conf`)

```conf
bind = $mainMod, Return, exec, estedad-term
```

---

## Environment Variables

| Variable | Values | Description |
|---|---|---|
| `ESTEDAD_GPU` | `intel`, `nvidia`, `default` | Forces GPU adapter selection (default: low-power adapter). |
| `WGPU_BACKEND` | `vulkan`, `gl` | Selects underlying graphics backend (default: Vulkan). |

---

## Testing

Run the test suite:

```bash
cargo test
```

All integration and unit tests cover text shaping, command transformation, GPU vertex generation, and PTY concurrency.

---

## License

This project is licensed under the [MIT License](LICENSE).
