use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cosmic_text::{Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
mod gpu;
use gpu::GpuRenderer;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{CursorIcon, Window, WindowId};

const DEFAULT_FONT_SIZE: f32 = 19.0;
const DEFAULT_LINE_HEIGHT: f32 = 29.0;
const MIN_FONT_SIZE: f32 = 8.0;
const MAX_FONT_SIZE: f32 = 48.0;
const PAD_X: f32 = 20.0;
const PAD_Y: f32 = 20.0;

enum AppEvent {
    PtyOutput,
    CommandFinished,
}

struct RunningCommand {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    #[allow(dead_code)]
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MousePos {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SelectionMode {
    Char,
    Word { orig_start: MousePos, orig_end: MousePos },
    Line { orig_start: MousePos, orig_end: MousePos },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SelectionState {
    None,
    Selecting {
        mode: SelectionMode,
        anchor: MousePos,
        current: MousePos,
    },
    Selected {
        start: MousePos,
        end: MousePos,
    },
}

impl SelectionState {
    fn get_points(&self, line_height: f32) -> Option<(MousePos, MousePos)> {
        match *self {
            SelectionState::None => None,
            SelectionState::Selecting { mode, anchor, current } => match mode {
                SelectionMode::Char => {
                    let dx = (current.x - anchor.x).abs();
                    let dy = (current.y - anchor.y).abs();
                    if dx > 2.0 || dy > 2.0 {
                        Some((anchor, current))
                    } else {
                        None
                    }
                }
                SelectionMode::Word { orig_start, orig_end } => {
                    let dx = (current.x - anchor.x).abs();
                    let dy = (current.y - anchor.y).abs();
                    if dx <= 5.0 && dy <= 5.0 {
                        Some((orig_start, orig_end))
                    } else {
                        if current.y > orig_end.y || (current.y >= orig_start.y && current.x >= orig_start.x) {
                            Some((orig_start, current))
                        } else {
                            Some((orig_end, current))
                        }
                    }
                }
                SelectionMode::Line { orig_start, orig_end } => {
                    let dy = (current.y - anchor.y).abs();
                    if dy <= line_height / 2.0 {
                        Some((orig_start, orig_end))
                    } else {
                        Some((orig_start, current))
                    }
                }
            },
            SelectionState::Selected { start, end } => Some((start, end)),
        }
    }
}

#[derive(Clone)]
enum HistoryEntry {
    Persian {
        text: String,
        color: Color,
        visual_lines: usize,
    },
    Monospace {
        spans: Vec<(usize, String, Color)>,
    },
}

impl HistoryEntry {
    fn visual_lines(&self) -> usize {
        match self {
            HistoryEntry::Persian { visual_lines, .. } => *visual_lines,
            HistoryEntry::Monospace { .. } => 1,
        }
    }
}

struct TerminalState {
    font_system: FontSystem,
    swash_cache: SwashCache,
    font_size: f32,
    line_height: f32,
    history: Vec<HistoryEntry>,
    current_input: String,
    cwd: PathBuf,
    username: String,
    hostname: String,
    start_time: Instant,
    last_cursor_visible: bool,
    modifiers: ModifiersState,
    scroll_offset: usize,
    scroll_pixel_accum: f32,
    running_command: Option<RunningCommand>,
    event_proxy: EventLoopProxy<AppEvent>,
    cell_width: f32,
    window_cols: u16,
    window_rows: u16,
    mouse_pos: Option<MousePos>,
    selection: SelectionState,
    last_click_time: Instant,
    last_click_pos: Option<MousePos>,
    click_count: usize,
    window_width: u32,
    window_height: u32,
    input_visual_lines: usize,
    cmd_history: Vec<String>,
    history_cursor: Option<usize>,
    saved_current_input: String,
}

struct PersianAlias {
    persian: &'static str,
    english: &'static str,
}

const PERSIAN_COMMANDS: &[PersianAlias] = &[
    // Multi-word / compound phrases (3 words)
    PersianAlias { persian: "پی دبلیو دی", english: "pwd" },
    PersianAlias { persian: "پی‌دبلیودی", english: "pwd" },
    PersianAlias { persian: "ای جی وای", english: "agy" },
    PersianAlias { persian: "ای‌جی‌وای", english: "agy" },
    PersianAlias { persian: "سیستم سی تی ال", english: "systemctl" },
    PersianAlias { persian: "سیستم‌سی‌تی‌ال", english: "systemctl" },
    PersianAlias { persian: "ژورنال سی تی ال", english: "journalctl" },
    PersianAlias { persian: "ژورنال‌سی‌تی‌ال", english: "journalctl" },

    // Multi-word phrases (2 words)
    PersianAlias { persian: "ال اس", english: "ls --color=auto" },
    PersianAlias { persian: "ال‌اس", english: "ls --color=auto" },
    PersianAlias { persian: "بی تاپ", english: "btop" },
    PersianAlias { persian: "بی‌تاپ", english: "btop" },
    PersianAlias { persian: "میک دیر", english: "mkdir -p" },
    PersianAlias { persian: "میک‌دیر", english: "mkdir -p" },
    PersianAlias { persian: "آر ام", english: "rm" },
    PersianAlias { persian: "آرام", english: "rm" },
    PersianAlias { persian: "آر‌ام", english: "rm" },
    PersianAlias { persian: "سی پی", english: "cp" },
    PersianAlias { persian: "سی‌پی", english: "cp" },
    PersianAlias { persian: "ام وی", english: "mv" },
    PersianAlias { persian: "ام‌وی", english: "mv" },
    PersianAlias { persian: "اس ال", english: "sl" },
    PersianAlias { persian: "اس‌ال", english: "sl" },
    PersianAlias { persian: "سی دی", english: "cd" },
    PersianAlias { persian: "سی‌دی", english: "cd" },
    PersianAlias { persian: "پاور آف", english: "poweroff" },
    PersianAlias { persian: "شات دان", english: "shutdown -h now" },
    PersianAlias { persian: "من کیم", english: "whoami" },
    PersianAlias { persian: "نام هاست", english: "hostname" },
    PersianAlias { persian: "ای پی", english: "ip" },
    PersianAlias { persian: "آی پی", english: "ip" },
    PersianAlias { persian: "آی‌پی", english: "ip" },
    PersianAlias { persian: "دی ان اف", english: "dnf" },
    PersianAlias { persian: "دی‌ان‌اف", english: "dnf" },

    // Single-word Fingilish & system tools
    PersianAlias { persian: "سودو", english: "sudo" },
    PersianAlias { persian: "نانو", english: "nano" },
    PersianAlias { persian: "کت", english: "cat" },
    PersianAlias { persian: "ویم", english: "vim" },
    PersianAlias { persian: "وی‌آی", english: "vi" },
    PersianAlias { persian: "گیت", english: "git" },
    PersianAlias { persian: "کلیر", english: "clear" },
    PersianAlias { persian: "سیدی", english: "cd" },
    PersianAlias { persian: "زی", english: "cd" },
    PersianAlias { persian: "پکمن", english: "pacman" },
    PersianAlias { persian: "اپت", english: "apt" },
    PersianAlias { persian: "کارگو", english: "cargo" },
    PersianAlias { persian: "داکر", english: "docker" },
    PersianAlias { persian: "پایتون", english: "python" },
    PersianAlias { persian: "پایتون۳", english: "python3" },
    PersianAlias { persian: "پینگ", english: "ping" },
    PersianAlias { persian: "بتاپ", english: "btop" },
    PersianAlias { persian: "تاپ", english: "top" },
    PersianAlias { persian: "اسلیپ", english: "sleep" },
    PersianAlias { persian: "کد", english: "code" },
    PersianAlias { persian: "اجی", english: "agy" },
    PersianAlias { persian: "کیل", english: "kill" },
    PersianAlias { persian: "اگزیت", english: "exit" },
    PersianAlias { persian: "اگسیت", english: "exit" },
    PersianAlias { persian: "راست", english: "rustc" },
    PersianAlias { persian: "ایکو", english: "echo" },
    PersianAlias { persian: "ریبوت", english: "reboot" },
    PersianAlias { persian: "ری‌بوت", english: "reboot" },
    PersianAlias { persian: "پاورآف", english: "poweroff" },

    // Git subcommands & development verbs
    PersianAlias { persian: "استاتوس", english: "status" },
    PersianAlias { persian: "کامیت", english: "commit" },
    PersianAlias { persian: "پوش", english: "push" },
    PersianAlias { persian: "پول", english: "pull" },
    PersianAlias { persian: "برنچ", english: "branch" },
    PersianAlias { persian: "شاخه", english: "branch" },
    PersianAlias { persian: "چکاوت", english: "checkout" },
    PersianAlias { persian: "چک‌اوت", english: "checkout" },
    PersianAlias { persian: "دیف", english: "diff" },
    PersianAlias { persian: "کلون", english: "clone" },
    PersianAlias { persian: "اد", english: "add" },
    PersianAlias { persian: "ریست", english: "reset" },
    PersianAlias { persian: "مرج", english: "merge" },
    PersianAlias { persian: "لاگ", english: "log" },

    // Service & package actions
    PersianAlias { persian: "ریستارت", english: "restart" },
    PersianAlias { persian: "ری‌استارت", english: "restart" },
    PersianAlias { persian: "استارت", english: "start" },
    PersianAlias { persian: "استاپ", english: "stop" },
    PersianAlias { persian: "اینستال", english: "install" },
    PersianAlias { persian: "نصب", english: "install" },
    PersianAlias { persian: "آپدیت", english: "update" },
    PersianAlias { persian: "اپدیت", english: "update" },
    PersianAlias { persian: "بروزرسانی", english: "update" },
    PersianAlias { persian: "آپگرید", english: "upgrade" },
    PersianAlias { persian: "اپگرید", english: "upgrade" },
    PersianAlias { persian: "ارتقا", english: "upgrade" },
    PersianAlias { persian: "سرچ", english: "search" },
    PersianAlias { persian: "جستجو", english: "search" },
    PersianAlias { persian: "بیلد", english: "build" },
    PersianAlias { persian: "ساختن", english: "build" },
    PersianAlias { persian: "ران", english: "run" },
    PersianAlias { persian: "اجرا", english: "run" },
    PersianAlias { persian: "تست", english: "test" },
    PersianAlias { persian: "چک", english: "check" },

    // Semantic Persian words
    PersianAlias { persian: "برو", english: "cd" },
    PersianAlias { persian: "برگرد", english: "cd .." },
    PersianAlias { persian: "خانه", english: "cd ~" },
    PersianAlias { persian: "لیست", english: "ls --color=auto" },
    PersianAlias { persian: "ببین", english: "ls --color=auto" },
    PersianAlias { persian: "پاک", english: "clear" },
    PersianAlias { persian: "تمیز", english: "clear" },
    PersianAlias { persian: "بخوان", english: "cat" },
    PersianAlias { persian: "نمایش", english: "cat" },
    PersianAlias { persian: "بساز", english: "mkdir -p" },
    PersianAlias { persian: "حذف", english: "rm -rf" },
    PersianAlias { persian: "ویرایش", english: "nano" },
    PersianAlias { persian: "مسیر", english: "pwd" },
    PersianAlias { persian: "خروج", english: "exit" },
    PersianAlias { persian: "تاریخ", english: "date" },
    PersianAlias { persian: "کی‌ام", english: "whoami" },
    PersianAlias { persian: "هاست‌نیم", english: "hostname" },
    PersianAlias { persian: "کپی", english: "cp" },
    PersianAlias { persian: "انتقال", english: "mv" },
];

fn persian_char_to_qwerty(c: char) -> Option<char> {
    match c {
        'ض' => Some('q'),
        'ص' => Some('w'),
        'ث' => Some('e'),
        'ق' => Some('r'),
        'ف' => Some('t'),
        'غ' => Some('y'),
        'ع' => Some('u'),
        'ه' => Some('i'),
        'خ' => Some('o'),
        'ح' => Some('p'),
        'ج' => Some('['),
        'چ' => Some(']'),
        'ش' => Some('a'),
        'س' => Some('s'),
        'ی' | 'ئ' | 'ي' => Some('d'),
        'ب' => Some('f'),
        'ل' => Some('g'),
        'ا' | 'آ' | 'أ' | 'إ' => Some('h'),
        'ت' => Some('j'),
        'ن' => Some('k'),
        'م' => Some('l'),
        'ک' | 'ك' => Some(';'),
        'گ' => Some('\''),
        'ظ' => Some('z'),
        'ط' => Some('x'),
        'ز' => Some('c'),
        'ژ' => Some('C'),
        'ر' => Some('v'),
        'ذ' => Some('b'),
        'د' => Some('n'),
        'پ' => Some('m'),
        'و' => Some(','),
        'ة' => Some('j'),
        'ؤ' => Some('w'),
        'ء' => Some('m'),
        '؛' => Some(';'),
        '،' => Some(','),
        '«' => Some('<'),
        '»' => Some('>'),
        '؟' => Some('?'),
        '÷' => Some('/'),
        'ـ' => Some('_'),
        _ => None,
    }
}

fn normalize_persian_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '۰' | '٠' => '0',
            '۱' | '١' => '1',
            '۲' | '٢' => '2',
            '۳' | '٣' => '3',
            '۴' | '٤' => '4',
            '۵' | '٥' => '5',
            '۶' | '٦' => '6',
            '۷' | '٧' => '7',
            '۸' | '٨' => '8',
            '۹' | '٩' => '9',
            '٫' => '.',
            other => other,
        })
        .collect()
}

fn normalize_persian_input(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '۰' | '٠' => '0',
            '۱' | '١' => '1',
            '۲' | '٢' => '2',
            '۳' | '٣' => '3',
            '۴' | '٤' => '4',
            '۵' | '٥' => '5',
            '۶' | '٦' => '6',
            '۷' | '٧' => '7',
            '۸' | '٨' => '8',
            '۹' | '٩' => '9',
            '٫' => '.',
            '؛' => ';',
            other => other,
        })
        .collect()
}

fn is_command_wrapper(cmd: &str) -> bool {
    matches!(
        cmd,
        "sudo" | "doas" | "env" | "time" | "nohup" | "xargs" | "exec" | "busybox"
    )
}

fn is_known_subcommand(sub: &str) -> bool {
    matches!(
        sub,
        "status"
            | "commit"
            | "push"
            | "pull"
            | "branch"
            | "checkout"
            | "diff"
            | "clone"
            | "add"
            | "reset"
            | "merge"
            | "log"
            | "fetch"
            | "rebase"
            | "tag"
            | "stash"
            | "remote"
            | "show"
            | "init"
            | "start"
            | "stop"
            | "restart"
            | "reload"
            | "enable"
            | "disable"
            | "mask"
            | "unmask"
            | "install"
            | "update"
            | "upgrade"
            | "search"
            | "remove"
            | "autoremove"
            | "clean"
            | "info"
            | "build"
            | "run"
            | "test"
            | "check"
            | "bench"
            | "new"
            | "publish"
            | "ps"
            | "images"
            | "exec"
            | "logs"
            | "attach"
            | "compose"
            | "up"
            | "down"
            | "list"
            | "edit"
            | "delete"
            | "create"
            | "help"
            | "version"
    )
}

fn is_executable_command(cmd: &str) -> bool {
    if matches!(
        cmd,
        "cd" | "exit" | "clear" | "echo" | "printf" | "export" | "set" | "unset"
            | "alias" | "source" | "history" | "help" | "jobs" | "fg" | "bg"
            | "true" | "false" | "test" | "eval" | "exec" | "read" | "type"
            | "sudo" | "doas" | "nano" | "vim" | "vi" | "cat" | "ls" | "pwd"
            | "cp" | "mv" | "rm" | "mkdir" | "rmdir" | "touch" | "grep" | "find"
            | "sed" | "awk" | "reboot" | "poweroff" | "shutdown" | "btop" | "htop"
            | "top" | "ps" | "kill" | "pkill" | "systemctl" | "journalctl" | "git"
            | "cargo" | "rustc" | "python" | "python3" | "node" | "npm" | "docker"
            | "pacman" | "apt" | "dnf" | "ping" | "curl" | "wget" | "ssh" | "ip"
            | "df" | "du" | "free" | "uname" | "whoami" | "hostname" | "sl" | "agy"
            | "code"
    ) {
        return true;
    }
    std::path::Path::new(&format!("/usr/bin/{}", cmd)).exists()
        || std::path::Path::new(&format!("/bin/{}", cmd)).exists()
        || std::path::Path::new(&format!("/usr/local/bin/{}", cmd)).exists()
}

fn decode_persian_token(token: &str) -> Option<String> {
    if !token.chars().any(is_persian_char) {
        return None;
    }
    let mut decoded = String::new();
    for c in token.chars() {
        if c == '\u{200C}' {
            continue;
        }
        if c.is_ascii() {
            decoded.push(c);
        } else if let Some(q) = persian_char_to_qwerty(c) {
            decoded.push(q);
        } else {
            return None;
        }
    }
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn parse_tokens(s: &str) -> Vec<(String, bool)> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let quote_char = chars[i];
        if quote_char == '"' || quote_char == '\'' {
            let mut val = String::new();
            val.push(quote_char);
            i += 1;
            while i < chars.len() && chars[i] != quote_char {
                val.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && chars[i] == quote_char {
                val.push(quote_char);
                i += 1;
            }
            tokens.push((val, true));
        } else {
            let mut val = String::new();
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '"' && chars[i] != '\'' {
                val.push(chars[i]);
                i += 1;
            }
            tokens.push((val, false));
        }
    }
    tokens
}

fn translate_single_command(cmd: &str) -> String {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Exact match for entire command phrase
    for alias in PERSIAN_COMMANDS {
        if trimmed == alias.persian {
            return alias.english.to_string();
        }
    }

    let raw_tokens = parse_tokens(trimmed);
    if raw_tokens.is_empty() {
        return String::new();
    }

    let mut translated_tokens: Vec<String> = Vec::new();
    let mut i = 0;
    let mut is_in_echo = false;

    while i < raw_tokens.len() {
        if is_in_echo {
            translated_tokens.push(raw_tokens[i].0.clone());
            i += 1;
            continue;
        }

        if raw_tokens[i].1 {
            // Quoted token: preserved verbatim
            translated_tokens.push(raw_tokens[i].0.clone());
            i += 1;
            continue;
        }

        // Check 3-word phrase match
        if i + 3 <= raw_tokens.len() && !raw_tokens[i + 1].1 && !raw_tokens[i + 2].1 {
            let phrase3 = format!("{} {} {}", raw_tokens[i].0, raw_tokens[i + 1].0, raw_tokens[i + 2].0);
            if let Some(alias) = PERSIAN_COMMANDS.iter().find(|a| a.persian == phrase3) {
                translated_tokens.push(alias.english.to_string());
                i += 3;
                continue;
            }
        }

        // Check 2-word phrase match
        if i + 2 <= raw_tokens.len() && !raw_tokens[i + 1].1 {
            let phrase2 = format!("{} {}", raw_tokens[i].0, raw_tokens[i + 1].0);
            if let Some(alias) = PERSIAN_COMMANDS.iter().find(|a| a.persian == phrase2) {
                translated_tokens.push(alias.english.to_string());
                i += 2;
                continue;
            }
        }

        // Single token match against PERSIAN_COMMANDS
        let tok = &raw_tokens[i].0;
        if let Some(alias) = PERSIAN_COMMANDS.iter().find(|a| a.persian == tok) {
            translated_tokens.push(alias.english.to_string());
            if alias.english == "echo" || alias.english == "printf" {
                is_in_echo = true;
            }
            i += 1;
            continue;
        }

        // Check keyboard layout typo decoding
        if let Some(decoded) = decode_persian_token(tok) {
            let is_cmd_pos = translated_tokens.is_empty()
                || is_command_wrapper(translated_tokens.last().map(|s| s.as_str()).unwrap_or(""));

            let is_flag = tok.starts_with('-');
            let is_sub = is_known_subcommand(&decoded);
            let is_path = (decoded.starts_with('/') || decoded.starts_with("./") || decoded.starts_with("../"))
                && (std::path::Path::new(&decoded).exists()
                    || std::path::Path::new(&decoded).parent().map_or(false, |p| p.as_os_str().is_empty() || p.exists()));

            if is_cmd_pos || is_flag || is_sub || is_path || is_executable_command(&decoded) {
                if decoded == "echo" || decoded == "printf" {
                    is_in_echo = true;
                }
                translated_tokens.push(decoded);
                i += 1;
                continue;
            }
        }

        // Unmatched token (e.g. Persian arguments, filenames)
        translated_tokens.push(tok.clone());
        i += 1;
    }

    let mut result = translated_tokens.join(" ");

    // Auto-color for ls command
    if result == "ls" {
        result = "ls --color=auto".to_string();
    } else if result.starts_with("ls ") && !result.contains("--color") {
        result = format!("ls --color=auto {}", &result[3..]);
    } else if result == "sudo ls" {
        result = "sudo ls --color=auto".to_string();
    } else if result.starts_with("sudo ls ") && !result.contains("--color") {
        result = format!("sudo ls --color=auto {}", &result[8..]);
    }

    result
}

fn split_shell_pipeline(cmd: &str) -> Vec<(String, String)> {
    let mut segments = Vec::new();
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    let mut current_segment = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            current_segment.push(c);
            i += 1;
        } else if c == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            current_segment.push(c);
            i += 1;
        } else if !in_single_quote && !in_double_quote {
            if c == '&' && i + 1 < chars.len() && chars[i + 1] == '&' {
                segments.push((current_segment.trim().to_string(), "&&".to_string()));
                current_segment.clear();
                i += 2;
            } else if c == '|' && i + 1 < chars.len() && chars[i + 1] == '|' {
                segments.push((current_segment.trim().to_string(), "||".to_string()));
                current_segment.clear();
                i += 2;
            } else if c == ';' {
                segments.push((current_segment.trim().to_string(), ";".to_string()));
                current_segment.clear();
                i += 1;
            } else if c == '|' {
                segments.push((current_segment.trim().to_string(), "|".to_string()));
                current_segment.clear();
                i += 1;
            } else {
                current_segment.push(c);
                i += 1;
            }
        } else {
            current_segment.push(c);
            i += 1;
        }
    }

    segments.push((current_segment.trim().to_string(), String::new()));
    segments
}

fn translate_persian_command(cmd: &str) -> String {
    let normalized = normalize_persian_input(cmd);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let segments = split_shell_pipeline(trimmed);
    let mut out = String::new();

    for (seg, op) in segments {
        if seg.is_empty() && op.is_empty() {
            continue;
        }
        let translated_seg = translate_single_command(&seg);
        if !op.is_empty() {
            out.push_str(&format!("{} {} ", translated_seg, op));
        } else {
            out.push_str(&translated_seg);
        }
    }

    out.trim().to_string()
}

fn clean_bidi_text(s: &str) -> String {
    s.chars()
        .filter(|&c| c != '\u{200F}' && c != '\u{2066}' && c != '\u{2069}')
        .collect()
}

fn monospace_spans_to_string(spans: &[(usize, String, Color)]) -> String {
    let mut result = String::new();
    let mut cur_col = 0;
    for (col, text, _) in spans {
        if *col > cur_col {
            result.push_str(&" ".repeat(*col - cur_col));
            cur_col = *col;
        }
        result.push_str(text);
        cur_col += text.chars().count();
    }
    result
}

fn is_word_separator(c: char) -> bool {
    c.is_whitespace()
        || c == '['
        || c == ']'
        || c == '('
        || c == ')'
        || c == '{'
        || c == '}'
        || c == '<'
        || c == '>'
        || c == '◀'
        || c == '▶'
        || c == ':'
        || c == ';'
        || c == '@'
        || c == '$'
        || c == '"'
        || c == '\''
        || c == '`'
        || c == '='
        || c == '|'
        || c == '&'
        || c == '!'
        || c == '?'
        || c == ','
        || c == '\u{200F}'
        || c == '\u{2066}'
        || c == '\u{2069}'
}

#[derive(Debug)]
enum LineRepresentation {
    Monospace(String),
    Persian {
        text: String,
        sub_line: usize,
    },
}

#[derive(Clone, Debug)]
struct GlyphHitbox {
    x1: f32,
    x2: f32,
    start: usize,
    end: usize,
}

fn get_mode2_line(
    history: &[HistoryEntry],
    prompt: &str,
    current_input: &str,
    vl: usize,
) -> LineRepresentation {
    let mut current_v = 0;
    for entry in history {
        let lines = entry.visual_lines();
        if vl < current_v + lines {
            let sub_line = vl - current_v;
            return match entry {
                HistoryEntry::Monospace { spans } => {
                    LineRepresentation::Monospace(monospace_spans_to_string(spans))
                }
                HistoryEntry::Persian { text, .. } => {
                    LineRepresentation::Persian {
                        text: text.clone(),
                        sub_line,
                    }
                }
            };
        }
        current_v += lines;
    }
    let sub_line = vl.saturating_sub(current_v);
    let full_line = format!("{}{}", prompt, current_input);
    LineRepresentation::Persian {
        text: full_line,
        sub_line,
    }
}

fn get_glyphs_for_line(
    font_system: &mut FontSystem,
    metrics: Metrics,
    avail_width: f32,
    cell_w: f32,
    line: &LineRepresentation,
) -> (String, Vec<GlyphHitbox>) {
    match line {
        LineRepresentation::Monospace(full_str) => {
            let mut hitboxes = Vec::new();
            let mut col = 0;
            for (byte_idx, ch) in full_str.char_indices() {
                let char_len = ch.len_utf8();
                let x1 = PAD_X + (col as f32) * cell_w;
                let x2 = x1 + cell_w;
                hitboxes.push(GlyphHitbox {
                    x1,
                    x2,
                    start: byte_idx,
                    end: byte_idx + char_len,
                });
                col += 1;
            }
            (full_str.clone(), hitboxes)
        }
        LineRepresentation::Persian { text, sub_line } => {
            let mut buf = Buffer::new(font_system, metrics);
            buf.set_size(Some(avail_width), None);
            buf.set_wrap(Wrap::Glyph);
            let attrs = Attrs::new().family(Family::Name("Estedad"));
            buf.set_text(text, &attrs, Shaping::Advanced, Some(Align::Right));
            buf.shape_until_scroll(font_system, false);

            let mut hitboxes = Vec::new();
            let runs: Vec<_> = buf.layout_runs().collect();
            let run_opt = runs.get(*sub_line).or_else(|| runs.last());
            if let Some(run) = run_opt {
                for g in run.glyphs.iter() {
                    let gx1 = PAD_X + g.x;
                    let gx2 = gx1 + g.w;
                    hitboxes.push(GlyphHitbox {
                        x1: gx1.min(gx2),
                        x2: gx1.max(gx2),
                        start: g.start,
                        end: g.end,
                    });
                }
            }
            (text.clone(), hitboxes)
        }
    }
}

impl TerminalState {
    fn new(event_proxy: EventLoopProxy<AppEvent>) -> Self {
        let mut font_system = FontSystem::new();

        // 1. Estedad fonts (Primary for Persian / Arabic cursive script)
        let estedad_paths = [
            "/usr/share/fonts/TTF/Estedad-Regular.ttf",
            "/usr/share/fonts/TTF/Estedad-Medium.ttf",
            "/usr/share/fonts/TTF/Estedad-SemiBold.ttf",
            "/usr/share/fonts/TTF/Estedad-Bold.ttf",
        ];
        for path in estedad_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }

        // 2. CaskaydiaCove Nerd Font Mono (Monospace grid, Braille, Icons, Terminal Symbols)
        let caskaydia_paths = [
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Regular.ttf",
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Bold.ttf",
        ];
        for path in caskaydia_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }

        // 3. DejaVu Sans (Box-drawing fallback)
        let dejavu_paths = [
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        ];
        for path in dejavu_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }

        let font_size = DEFAULT_FONT_SIZE;
        let line_height = DEFAULT_LINE_HEIGHT;
        let cell_width = measure_cell_advance(&mut font_system, font_size);

        let username = std::env::var("USER").unwrap_or_else(|_| "farzad".to_string());
        let hostname = std::fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "cachyos".to_string());
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

        Self {
            font_system,
            swash_cache: SwashCache::new(),
            font_size,
            line_height,
            history: Vec::new(),
            current_input: String::new(),
            cwd,
            username,
            hostname,
            start_time: Instant::now(),
            last_cursor_visible: true,
            modifiers: ModifiersState::default(),
            scroll_offset: 0,
            scroll_pixel_accum: 0.0,
            running_command: None,
            event_proxy,
            cell_width,
            window_cols: 90,
            window_rows: 28,
            mouse_pos: None,
            selection: SelectionState::None,
            last_click_time: Instant::now(),
            last_click_pos: None,
            click_count: 0,
            window_width: 1024,
            window_height: 768,
            input_visual_lines: 1,
            cmd_history: {
                let mut hist: Vec<String> = Vec::new();
                if let Ok(home) = std::env::var("HOME") {
                    let bash_hist_path = std::path::Path::new(&home).join(".bash_history");
                    if let Ok(content) = std::fs::read_to_string(&bash_hist_path) {
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() && hist.last().map(|s| s.as_str()) != Some(trimmed) {
                                hist.push(trimmed.to_string());
                            }
                        }
                    }
                }
                if hist.len() > 1000 {
                    let start = hist.len() - 1000;
                    hist = hist.split_off(start);
                }
                hist
            },
            history_cursor: None,
            saved_current_input: String::new(),
        }
    }

    fn history_up(&mut self) {
        if self.cmd_history.is_empty() {
            return;
        }
        match self.history_cursor {
            None => {
                self.saved_current_input = self.current_input.clone();
                let last_idx = self.cmd_history.len() - 1;
                self.history_cursor = Some(last_idx);
                self.current_input = self.cmd_history[last_idx].clone();
            }
            Some(idx) => {
                if idx > 0 {
                    let new_idx = idx - 1;
                    self.history_cursor = Some(new_idx);
                    self.current_input = self.cmd_history[new_idx].clone();
                }
            }
        }
    }

    fn history_down(&mut self) {
        match self.history_cursor {
            None => {}
            Some(idx) => {
                if idx + 1 < self.cmd_history.len() {
                    let new_idx = idx + 1;
                    self.history_cursor = Some(new_idx);
                    self.current_input = self.cmd_history[new_idx].clone();
                } else {
                    self.history_cursor = None;
                    self.current_input = std::mem::take(&mut self.saved_current_input);
                }
            }
        }
    }

    fn push_cmd_history(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        if !trimmed.is_empty() {
            if self.cmd_history.last().map(|s| s.as_str()) != Some(trimmed) {
                self.cmd_history.push(trimmed.to_string());
                if let Ok(home) = std::env::var("HOME") {
                    use std::io::Write;
                    let bash_hist_path = std::path::Path::new(&home).join(".bash_history");
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&bash_hist_path) {
                        let _ = writeln!(f, "{}", trimmed);
                    }
                }
            }
        }
        self.history_cursor = None;
        self.saved_current_input.clear();
    }

    fn current_prompt(&self) -> String {
        let home = std::env::var("HOME").unwrap_or_default();
        let path_str = self.cwd.to_string_lossy();
        let display_path = if !home.is_empty() && path_str.starts_with(&home) {
            format!("~{}", &path_str[home.len()..])
        } else {
            path_str.to_string()
        };
        // RLM + LTR isolates: guarantees the line starts from the RIGHT edge in Persian BiDi,
        // while preserving natural English spelling of usernames and directories.
        format!("\u{200F}[\u{2066}{}@{}\u{2069}: \u{2066}{}\u{2069}] $ ", self.username, self.hostname, display_path)
    }

    fn execute_command(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return;
        }

        let translated = translate_persian_command(trimmed);

        if translated == "exit" {
            std::process::exit(0);
        }

        if translated == "clear" {
            self.history.clear();
            return;
        }

        if translated == "cd" || translated.starts_with("cd ") {
            let target = if translated == "cd" {
                std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
            } else {
                translated[3..].trim().to_string()
            };

            let target_path = if target.starts_with('~') {
                let home = std::env::var("HOME").unwrap_or_default();
                PathBuf::from(target.replacen('~', &home, 1))
            } else if target.starts_with('/') {
                PathBuf::from(target)
            } else {
                self.cwd.join(target)
            };

            if let Ok(canon) = target_path.canonicalize() {
                if canon.is_dir() {
                    let _ = std::env::set_current_dir(&canon);
                    self.cwd = canon;
                } else {
                    self.push_persian_history(
                        "\u{200F}خطا: مسیر وارد شده یک پوشه نیست".to_string(),
                        Color::rgb(255, 120, 120),
                    );
                }
            } else {
                self.push_persian_history(
                    format!("\u{200F}cd: پوشه یافت نشد: \u{2066}{}\u{2069}", target_path.display()),
                    Color::rgb(255, 120, 120),
                );
            }
            return;
        }

        // Spawn command inside a real PTY so interactive commands (sl, btop, agy, nano) run live
        let pty_system = native_pty_system();
        let pair = match pty_system.openpty(PtySize {
            rows: self.window_rows,
            cols: self.window_cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                self.push_persian_history(
                    format!("\u{200F}خطا در باز کردن PTY: {}", e),
                    Color::rgb(255, 120, 120),
                );
                return;
            }
        };

        let mut cmd_builder = CommandBuilder::new("/bin/bash");
        cmd_builder.args(["-c", &translated]);
        cmd_builder.cwd(&self.cwd);
        cmd_builder.env("TERM", "xterm-256color");
        cmd_builder.env("COLORTERM", "truecolor");

        let child = match pair.slave.spawn_command(cmd_builder) {
            Ok(c) => c,
            Err(e) => {
                self.push_persian_history(
                    format!("\u{200F}خطا در اجرای دستور: {}", e),
                    Color::rgb(255, 120, 120),
                );
                return;
            }
        };
        drop(pair.slave); // Crucial for EOF detection

        let reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let master = pair.master;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(self.window_rows, self.window_cols, 2000)));

        // Reader thread: streams output from PTY into VT100 parser
        let parser_clone = Arc::clone(&parser);
        let proxy_chunk = self.event_proxy.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 8192];
            let mut filter = CsiStreamFilter::default();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                filter.filter(&mut buf[..n]);
                {
                    let mut p = parser_clone.lock().unwrap();
                    p.process(&buf[..n]);
                }
                let _ = proxy_chunk.send_event(AppEvent::PtyOutput);
            }
        });

        // Child process watcher thread: notifies when command finishes
        let child_arc = Arc::new(Mutex::new(child));
        let child_watcher = Arc::clone(&child_arc);
        let proxy_finish = self.event_proxy.clone();
        std::thread::spawn(move || {
            if let Ok(mut c) = child_watcher.lock() {
                let _ = c.wait();
            }
            let _ = proxy_finish.send_event(AppEvent::CommandFinished);
        });

        self.running_command = Some(RunningCommand {
            parser,
            writer: Arc::new(Mutex::new(writer)),
            master: Arc::new(Mutex::new(master)),
            child: child_arc,
        });
    }

    fn write_to_pty(&self, bytes: &[u8]) {
        if let Some(cmd) = &self.running_command {
            if let Ok(mut w) = cmd.writer.lock() {
                let _ = w.write_all(bytes);
                let _ = w.flush();
            }
        }
    }

    fn set_font_size(&mut self, new_size: f32) {
        let new_size = new_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if (self.font_size - new_size).abs() < 0.001 {
            return;
        }
        self.font_size = new_size;
        self.line_height = (new_size * (DEFAULT_LINE_HEIGHT / DEFAULT_FONT_SIZE)).round().max(10.0);
        self.cell_width = measure_cell_advance(&mut self.font_system, self.font_size);
        self.selection = SelectionState::None;
        self.recalculate_history_lines();

        let (new_rows, new_cols) = compute_grid_size(
            self.window_width,
            self.window_height,
            self.cell_width,
            self.line_height,
        );
        self.window_rows = new_rows;
        self.window_cols = new_cols;

        if let Some(cmd) = &self.running_command {
            if let Ok(master) = cmd.master.lock() {
                let _ = master.resize(PtySize {
                    rows: new_rows,
                    cols: new_cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
            if let Ok(mut parser) = cmd.parser.lock() {
                parser.screen_mut().set_size(new_rows, new_cols);
            }
        }
    }

    fn change_font_size(&mut self, delta: f32) {
        self.set_font_size(self.font_size + delta);
    }

    fn reset_font_size(&mut self) {
        self.set_font_size(DEFAULT_FONT_SIZE);
    }

    fn push_persian_history(&mut self, text: String, color: Color) {
        let avail_width = (self.window_width as f32 - (PAD_X * 2.0)).max(10.0);
        let metrics = Metrics::new(self.font_size, self.line_height);
        let mut buf = Buffer::new(&mut self.font_system, metrics);
        buf.set_size(Some(avail_width), None);
        buf.set_wrap(Wrap::Glyph);
        let attrs = Attrs::new().family(Family::Name("Estedad"));
        buf.set_text(&text, &attrs, Shaping::Advanced, Some(Align::Right));
        buf.shape_until_scroll(&mut self.font_system, false);
        let visual_lines = buf.layout_runs().count().max(1);

        self.history.push(HistoryEntry::Persian {
            text,
            color,
            visual_lines,
        });
    }

    fn recalculate_history_lines(&mut self) {
        let avail_width = (self.window_width as f32 - (PAD_X * 2.0)).max(10.0);
        let metrics = Metrics::new(self.font_size, self.line_height);
        for entry in &mut self.history {
            if let HistoryEntry::Persian { text, visual_lines, .. } = entry {
                let mut buf = Buffer::new(&mut self.font_system, metrics);
                buf.set_size(Some(avail_width), None);
                buf.set_wrap(Wrap::Glyph);
                let attrs = Attrs::new().family(Family::Name("Estedad"));
                buf.set_text(text, &attrs, Shaping::Advanced, Some(Align::Right));
                buf.shape_until_scroll(&mut self.font_system, false);
                *visual_lines = buf.layout_runs().count().max(1);
            }
        }

        let prompt = self.current_prompt();
        let full_line = format!("{}{}", prompt, self.current_input);
        let mut buf = Buffer::new(&mut self.font_system, metrics);
        buf.set_size(Some(avail_width), None);
        buf.set_wrap(Wrap::Glyph);
        let attrs = Attrs::new().family(Family::Name("Estedad"));
        buf.set_text(&full_line, &attrs, Shaping::Advanced, Some(Align::Right));
        buf.shape_until_scroll(&mut self.font_system, false);
        self.input_visual_lines = buf.layout_runs().count().max(1);
    }

    fn current_mode2_scroll_offset_y(&self) -> i32 {
        let total_visual_lines = self.history.iter().map(|e| e.visual_lines()).sum::<usize>() + self.input_visual_lines;
        let line_spacing = self.line_height as usize;
        let total_content_height = total_visual_lines * line_spacing + 50;
        let window_h = if self.window_height > 0 { self.window_height as usize } else { 600 };
        let max_scroll_offset_y = if total_content_height > window_h {
            (total_content_height - window_h) as i32
        } else {
            0
        };
        let scroll_shift = (self.scroll_offset as f32 * self.line_height) as i32;
        (max_scroll_offset_y - scroll_shift).max(0)
    }

    fn screen_to_doc_pos(&self, screen_pos: MousePos) -> MousePos {
        if self.running_command.is_none() {
            let scroll_offset_y = self.current_mode2_scroll_offset_y();
            let doc_y = (screen_pos.y - PAD_Y) + (scroll_offset_y as f32);
            MousePos { x: screen_pos.x, y: doc_y.max(0.0) }
        } else {
            screen_pos
        }
    }

    fn get_selected_text(&mut self) -> String {
        let (p1, p2) = match self.selection.get_points(self.line_height) {
            Some(pts) => pts,
            None => return String::new(),
        };

        if let Some(cmd) = &self.running_command {
            extract_mode1_selection(&cmd.parser, self.cell_width, self.line_height, p1, p2)
        } else {
            extract_mode2_selection(self, p1, p2)
        }
    }

    fn select_word_at(&mut self, pos: MousePos) -> Option<(MousePos, MousePos)> {
        if let Some(cmd) = &self.running_command {
            let p = cmd.parser.lock().unwrap();
            let screen = p.screen();
            let (term_rows, term_cols) = screen.size();
            let r = (((pos.y - PAD_Y) / self.line_height).floor() as i32).clamp(0, term_rows as i32 - 1) as u16;
            let c = (((pos.x - PAD_X) / self.cell_width).floor() as i32).clamp(0, term_cols as i32 - 1) as u16;

            let is_word_char = |col: u16| -> bool {
                if let Some(cell) = screen.cell(r, col) {
                    let s = cell.contents();
                    if let Some(ch) = s.chars().next() {
                        !is_word_separator(ch) && ch != '\0'
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if !is_word_char(c) {
                return None;
            }

            let mut start_c = c;
            while start_c > 0 && is_word_char(start_c - 1) {
                start_c -= 1;
            }
            let mut end_c = c;
            while end_c + 1 < term_cols && is_word_char(end_c + 1) {
                end_c += 1;
            }

            let x1 = PAD_X + (start_c as f32) * self.cell_width;
            let x2 = PAD_X + ((end_c + 1) as f32) * self.cell_width;
            let y = PAD_Y + (r as f32) * self.line_height + self.line_height / 2.0;

            let p1 = MousePos { x: x1, y };
            let p2 = MousePos { x: x2, y };
            let text = extract_mode1_selection(&cmd.parser, self.cell_width, self.line_height, p1, p2);
            if !text.is_empty() {
                copy_to_clipboard(&text);
            }
            return Some((p1, p2));
        }

        // Mode 2 (Prompt and Shell History)
        let total_visual_lines = self.history.iter().map(|e| e.visual_lines()).sum::<usize>() + self.input_visual_lines;
        let scroll_offset_y = self.current_mode2_scroll_offset_y();
        let virt_y = ((pos.y as i32) - (PAD_Y as i32) + scroll_offset_y).max(0);
        let vl = ((virt_y as f32 / self.line_height).floor() as usize).min(total_visual_lines.saturating_sub(1));

        let avail_width = (self.window_width as f32 - (PAD_X * 2.0)).max(10.0);
        let metrics = Metrics::new(self.font_size, self.line_height);
        let prompt = self.current_prompt();
        let current_input = self.current_input.clone();
        let cell_w = self.cell_width;
        let line_rep = get_mode2_line(&self.history, &prompt, &current_input, vl);
        let (text, hitboxes) = get_glyphs_for_line(&mut self.font_system, metrics, avail_width, cell_w, &line_rep);

        if hitboxes.is_empty() {
            return None;
        }

        // Find the glyph under or closest to pos.x (within 40px)
        let clicked_glyph = hitboxes.iter().find(|h| pos.x >= h.x1 && pos.x <= h.x2)
            .or_else(|| {
                hitboxes.iter()
                    .filter(|h| (pos.x - (h.x1 + h.x2) / 2.0).abs() < 40.0)
                    .min_by(|a, b| {
                        let dist_a = (pos.x - (a.x1 + a.x2) / 2.0).abs();
                        let dist_b = (pos.x - (b.x1 + b.x2) / 2.0).abs();
                        dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
                    })
            });

        let g = match clicked_glyph {
            Some(g) => g,
            None => return None,
        };

        let char_indices: Vec<(usize, char)> = text.char_indices().collect();
        if char_indices.is_empty() {
            return None;
        }

        let click_idx = char_indices.iter().position(|&(idx, _)| idx >= g.start).unwrap_or(0);
        if is_word_separator(char_indices[click_idx].1) {
            return None;
        }

        let mut start_i = click_idx;
        while start_i > 0 && !is_word_separator(char_indices[start_i - 1].1) {
            start_i -= 1;
        }
        let mut end_i = click_idx;
        while end_i + 1 < char_indices.len() && !is_word_separator(char_indices[end_i + 1].1) {
            end_i += 1;
        }

        let byte_start = char_indices[start_i].0;
        let byte_end = char_indices[end_i].0 + char_indices[end_i].1.len_utf8();

        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        for h in &hitboxes {
            if h.end > byte_start && h.start < byte_end {
                min_x = min_x.min(h.x1);
                max_x = max_x.max(h.x2);
            }
        }

        if min_x.is_finite() && max_x.is_finite() {
            let doc_y = (vl as f32) * self.line_height + self.line_height / 2.0;
            let word_text = clean_bidi_text(&text[byte_start..byte_end]);
            if !word_text.is_empty() {
                copy_to_clipboard(&word_text);
            }
            Some((MousePos { x: min_x, y: doc_y }, MousePos { x: max_x, y: doc_y }))
        } else {
            None
        }
    }

    fn select_line_at(&mut self, pos: MousePos) -> Option<(MousePos, MousePos)> {
        if let Some(cmd) = &self.running_command {
            let p = cmd.parser.lock().unwrap();
            let screen = p.screen();
            let (term_rows, term_cols) = screen.size();
            let r = (((pos.y - PAD_Y) / self.line_height).floor() as i32).clamp(0, term_rows as i32 - 1) as u16;
            let y = PAD_Y + (r as f32) * self.line_height + self.line_height / 2.0;
            let x1 = PAD_X;
            let x2 = PAD_X + (term_cols as f32) * self.cell_width;
            let p1 = MousePos { x: x1, y };
            let p2 = MousePos { x: x2, y };
            let text = extract_mode1_selection(&cmd.parser, self.cell_width, self.line_height, p1, p2);
            if !text.is_empty() {
                copy_to_clipboard(&text);
            }
            return Some((p1, p2));
        }

        // Mode 2:
        let total_visual_lines = self.history.iter().map(|e| e.visual_lines()).sum::<usize>() + self.input_visual_lines;
        let scroll_offset_y = self.current_mode2_scroll_offset_y();
        let virt_y = ((pos.y as i32) - (PAD_Y as i32) + scroll_offset_y).max(0);
        let vl = ((virt_y as f32 / self.line_height).floor() as usize).min(total_visual_lines.saturating_sub(1));
        let doc_y = (vl as f32) * self.line_height + self.line_height / 2.0;

        let right = self.window_width as f32 - PAD_X;
        let p1 = MousePos { x: PAD_X, y: doc_y };
        let p2 = MousePos { x: right, y: doc_y };
        let text = extract_mode2_selection(self, p1, p2);
        if !text.is_empty() {
            copy_to_clipboard(&text);
        }
        Some((p1, p2))
    }
}

fn copy_to_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }
    let s = text.to_string();
    std::thread::spawn(move || {
        if let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(s.as_bytes());
            }
            let _ = child.wait();
            return;
        }
        if let Ok(mut child) = std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(s.as_bytes());
            }
            let _ = child.wait();
        }
    });
}

fn paste_from_clipboard() -> Option<String> {
    if let Ok(output) = std::process::Command::new("wl-paste")
        .args(["--no-newline"])
        .output()
    {
        if output.status.success() {
            if let Ok(s) = String::from_utf8(output.stdout) {
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    if let Ok(output) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
    {
        if output.status.success() {
            if let Ok(s) = String::from_utf8(output.stdout) {
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn extract_mode1_selection(
    parser: &Arc<Mutex<vt100::Parser>>,
    cell_w: f32,
    line_h: f32,
    p1: MousePos,
    p2: MousePos,
) -> String {
    let p = parser.lock().unwrap();
    let screen = p.screen();
    let (term_rows, term_cols) = screen.size();

    let r1 = (((p1.y - PAD_Y) / line_h).floor() as i32).clamp(0, term_rows as i32 - 1) as u16;
    let c1 = (((p1.x - PAD_X) / cell_w).floor() as i32).clamp(0, term_cols as i32 - 1) as u16;

    let r2 = (((p2.y - PAD_Y) / line_h).floor() as i32).clamp(0, term_rows as i32 - 1) as u16;
    let c2 = (((p2.x - PAD_X) / cell_w).floor() as i32).clamp(0, term_cols as i32 - 1) as u16;

    let ((r_start, c_start), (r_end, c_end)) = if (r1, c1) <= (r2, c2) {
        ((r1, c1), (r2, c2))
    } else {
        ((r2, c2), (r1, c1))
    };

    let at_left_or_mid = r_start < r_end && c_end <= term_cols / 2;

    let mut result = String::new();
    for r in r_start..=r_end {
        let (col_min, col_max) = if r_start == r_end {
            (c_start, c_end)
        } else if r == r_start {
            (c_start, term_cols - 1)
        } else if r == r_end {
            if at_left_or_mid {
                (0, term_cols - 1)
            } else {
                (0, c_end)
            }
        } else {
            (0, term_cols - 1)
        };

        let mut row_str = String::new();
        for c in col_min..=col_max {
            if let Some(cell) = screen.cell(r, c) {
                if !cell.is_wide_continuation() {
                    let content = cell.contents();
                    row_str.push_str(if content.is_empty() { " " } else { content });
                }
            } else {
                row_str.push(' ');
            }
        }
        let trimmed = row_str.trim_end();
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(trimmed);
    }
    result
}

fn extract_mode2_selection(state: &mut TerminalState, p1: MousePos, p2: MousePos) -> String {
    let total_visual_lines = state.history.iter().map(|e| e.visual_lines()).sum::<usize>() + 1;
    if total_visual_lines == 0 {
        return String::new();
    }

    let l1 = ((p1.y / state.line_height).floor() as usize).min(total_visual_lines.saturating_sub(1));
    let l2 = ((p2.y / state.line_height).floor() as usize).min(total_visual_lines.saturating_sub(1));

    let ((l_start, x_start), (l_end, x_end)) = if l1 < l2 || (l1 == l2 && p1.x <= p2.x) {
        ((l1, p1.x), (l2, p2.x))
    } else {
        ((l2, p2.x), (l1, p1.x))
    };

    let avail_width = (state.window_width as f32 - (PAD_X * 2.0)).max(10.0);
    let metrics = Metrics::new(state.font_size, state.line_height);
    let prompt = state.current_prompt();
    let current_input = state.current_input.clone();
    let cell_w = state.cell_width;

    let mut result_lines = Vec::new();
    let mid_x = if state.window_width > 0 {
        state.window_width as f32 / 2.0
    } else {
        400.0
    };
    let at_left_or_mid = l_start < l_end && x_end <= mid_x;

    for l in l_start..=l_end {
        let (left, right) = if l_start == l_end {
            (x_start.min(x_end), x_start.max(x_end))
        } else if l == l_start {
            (x_start, f32::INFINITY)
        } else if l == l_end {
            if at_left_or_mid {
                (0.0, f32::INFINITY)
            } else {
                (0.0, x_end)
            }
        } else {
            (0.0, f32::INFINITY)
        };

        let line_rep = get_mode2_line(&state.history, &prompt, &current_input, l);
        let (text, hitboxes) = get_glyphs_for_line(&mut state.font_system, metrics, avail_width, cell_w, &line_rep);

        if hitboxes.is_empty() {
            continue;
        }

        let mut min_byte = usize::MAX;
        let mut max_byte = 0;
        let mut found = false;

        for h in &hitboxes {
            let center = (h.x1 + h.x2) / 2.0;
            if h.x2 > h.x1 && center >= left && center <= right {
                found = true;
                min_byte = min_byte.min(h.start);
                max_byte = max_byte.max(h.end);
            }
        }

        // Fallback for delicate or short drags
        if !found {
            for h in &hitboxes {
                if h.x2 > h.x1 && h.x2 > left && h.x1 < right {
                    found = true;
                    min_byte = min_byte.min(h.start);
                    max_byte = max_byte.max(h.end);
                }
            }
        }

        if found && min_byte < max_byte && max_byte <= text.len() {
            let extracted = clean_bidi_text(&text[min_byte..max_byte]);
            let trimmed = extracted.trim_end();
            if !trimmed.is_empty() {
                result_lines.push(trimmed.to_string());
            }
        }
    }

    result_lines.join("\n")
}

#[allow(dead_code)]
fn blend_rect(
    buffer: &mut [u32],
    screen_width: usize,
    screen_height: usize,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: u32,
    alpha: u32,
) {
    if x >= screen_width as i32 || y >= screen_height as i32 || x + (w as i32) <= 0 || y + (h as i32) <= 0 {
        return;
    }
    let ux = x.max(0) as usize;
    let uy = y.max(0) as usize;
    let x_end = ((x + w as i32).max(0) as usize).min(screen_width);
    let y_end = ((y + h as i32).max(0) as usize).min(screen_height);

    let cr = (color >> 16) & 0xFF;
    let cg = (color >> 8) & 0xFF;
    let cb = color & 0xFF;
    let inv_alpha = 255 - alpha;

    for py in uy..y_end {
        let row_offset = py * screen_width;
        for px in ux..x_end {
            let idx = row_offset + px;
            let dst = buffer[idx];
            let dr = (dst >> 16) & 0xFF;
            let dg = (dst >> 8) & 0xFF;
            let db = dst & 0xFF;

            let out_r = (cr * alpha + dr * inv_alpha) / 255;
            let out_g = (cg * alpha + dg * inv_alpha) / 255;
            let out_b = (cb * alpha + db * inv_alpha) / 255;

            buffer[idx] = 0xFF000000 | (out_r << 16) | (out_g << 8) | out_b;
        }
    }
}

fn measure_cell_advance(font_system: &mut FontSystem, font_size: f32) -> f32 {
    let metrics = Metrics::new(font_size, font_size * 1.5);
    let mut buffer = Buffer::new(font_system, metrics);
    let attrs = Attrs::new().family(Family::Name("CaskaydiaCove Nerd Font Mono"));
    buffer.set_text("M", &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    for run in buffer.layout_runs() {
        for g in run.glyphs.iter() {
            return g.w;
        }
    }
    11.13
}

fn is_persian_char(c: char) -> bool {
    matches!(
        c,
        '\u{0600}'..='\u{06FF}'
            | '\u{0750}'..='\u{077F}'
            | '\u{08A0}'..='\u{08FF}'
            | '\u{FB50}'..='\u{FDFF}'
            | '\u{FE70}'..='\u{FEFF}'
    )
}

fn extract_row_spans(screen: &vt100::Screen, row: u16, cols: u16) -> Vec<(usize, String, Color)> {
    let mut spans = Vec::new();
    let mut col = 0;
    while col < cols {
        if let Some(cell) = screen.cell(row, col) {
            if cell.is_wide_continuation() {
                col += 1;
                continue;
            }
            let content = cell.contents();
            if content.is_empty() || content == " " {
                col += 1;
                continue;
            }

            let start_col = col as usize;
            let span_fg = vt_to_cosmic(cell.fgcolor());
            let mut text = String::new();

            while col < cols {
                if let Some(c_cell) = screen.cell(row, col) {
                    if c_cell.is_wide_continuation() {
                        col += 1;
                        continue;
                    }
                    let c_content = c_cell.contents();
                    if c_content.is_empty() {
                        break;
                    }
                    let c_fg = vt_to_cosmic(c_cell.fgcolor());
                    if c_fg != span_fg {
                        break;
                    }
                    if c_content == " " {
                        // Check if next cell is also empty or space (column boundary: >= 2 spaces)
                        if col + 1 >= cols {
                            break;
                        }
                        if let Some(next_cell) = screen.cell(row, col + 1) {
                            let next_c = next_cell.contents();
                            if next_c.is_empty() || next_c == " " {
                                break;
                            }
                        }
                    }
                    text.push_str(if c_content.is_empty() { " " } else { c_content });
                    col += 1;
                } else {
                    break;
                }
            }

            let trimmed = text.trim_end();
            if !trimmed.is_empty() {
                spans.push((start_col, trimmed.to_string(), span_fg));
            }
        } else {
            col += 1;
        }
    }
    spans
}

fn row_has_persian(screen: &vt100::Screen, row: u16, cols: u16) -> bool {
    for col in 0..cols {
        if let Some(cell) = screen.cell(row, col) {
            for c in cell.contents().chars() {
                if is_persian_char(c) {
                    return true;
                }
            }
        }
    }
    false
}

fn row_has_block_element(screen: &vt100::Screen, row: u16, cols: u16) -> bool {
    for col in 0..cols {
        if let Some(cell) = screen.cell(row, col) {
            let ch = cell.contents().chars().next().unwrap_or(' ');
            if ('\u{2500}'..='\u{259F}').contains(&ch) {
                return true;
            }
        }
    }
    false
}

fn row_is_multicolumn(screen: &vt100::Screen, row: u16, cols: u16) -> bool {
    let spans = extract_row_spans(screen, row, cols);
    if spans.len() < 2 {
        return false;
    }
    for i in 0..spans.len() - 1 {
        let end_of_current = spans[i].0 + spans[i].1.chars().count();
        let start_of_next = spans[i + 1].0;
        if start_of_next.saturating_sub(end_of_current) >= 3 {
            return true;
        }
    }
    false
}

fn cell_colors(cell: &vt100::Cell) -> (Color, Option<Color>) {
    let raw_fg = cell.fgcolor();
    let raw_bg = cell.bgcolor();
    let default_fg = Color::rgb(220, 230, 242);
    let default_bg = Color::rgb(15, 17, 26);

    if cell.inverse() {
        let fg = match raw_bg {
            vt100::Color::Default => default_bg,
            c => vt_to_cosmic(c),
        };
        let bg = match raw_fg {
            vt100::Color::Default => default_fg,
            c => vt_to_cosmic(c),
        };
        (fg, Some(bg))
    } else {
        let fg = match raw_fg {
            vt100::Color::Default => default_fg,
            c => vt_to_cosmic(c),
        };
        let bg = match raw_bg {
            vt100::Color::Default => None,
            c => Some(vt_to_cosmic(c)),
        };
        (fg, bg)
    }
}

// Convert VT100 color to Cosmic Text Color
fn vt_to_cosmic(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::rgb(220, 230, 242), // Crisp white text
        vt100::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
        vt100::Color::Idx(idx) => ansi_idx_to_color(idx),
    }
}

fn ansi_idx_to_color(idx: u8) -> Color {
    match idx {
        0 => Color::rgb(30, 32, 48),     // Black
        1 => Color::rgb(247, 118, 142),  // Red
        2 => Color::rgb(158, 206, 106),  // Green
        3 => Color::rgb(224, 175, 104),  // Yellow
        4 => Color::rgb(122, 162, 247),  // Blue
        5 => Color::rgb(187, 154, 247),  // Magenta
        6 => Color::rgb(125, 207, 255),  // Cyan
        7 => Color::rgb(192, 202, 245),  // White
        8 => Color::rgb(86, 95, 137),    // Bright Black
        9 => Color::rgb(255, 122, 144),  // Bright Red
        10 => Color::rgb(169, 221, 115), // Bright Green
        11 => Color::rgb(240, 190, 118), // Bright Yellow
        12 => Color::rgb(137, 180, 250), // Bright Blue
        13 => Color::rgb(203, 166, 247), // Bright Magenta
        14 => Color::rgb(148, 226, 213), // Bright Cyan
        15 => Color::rgb(205, 214, 244), // Bright White
        16..=231 => {
            let n = idx - 16;
            let r = (n / 36) * 51;
            let g = ((n % 36) / 6) * 51;
            let b = (n % 6) * 51;
            Color::rgb(r, g, b)
        }
        232..=255 => {
            let gray = 8 + (idx - 232) * 10;
            Color::rgb(gray, gray, gray)
        }
    }
}

#[allow(dead_code)]
fn color_to_u32(c: Color) -> u32 {
    0xFF000000 | ((c.r() as u32) << 16) | ((c.g() as u32) << 8) | (c.b() as u32)
}

fn is_block_element(ch: char) -> bool {
    ('\u{2580}'..='\u{259F}').contains(&ch)
}

#[allow(dead_code)]
fn draw_block_element(
    buffer: &mut [u32],
    screen_width: usize,
    screen_height: usize,
    x_start: usize,
    x_end: usize,
    y_start: usize,
    y_end: usize,
    ch: char,
    fg: Color,
    bg: Option<Color>,
) {
    if x_start >= screen_width || y_start >= screen_height || x_start >= x_end || y_start >= y_end {
        return;
    }
    let x_end = x_end.min(screen_width);
    let y_end = y_end.min(screen_height);

    let fg_u32 = color_to_u32(fg);
    let bg_u32 = bg.map(color_to_u32).unwrap_or(0xFF0F111A);

    let w = x_end - x_start;
    let h = y_end - y_start;
    let x_mid = x_start + w / 2;
    let y_mid = y_start + h / 2;

    for py in y_start..y_end {
        let is_top = py < y_mid;
        let row_offset = py * screen_width;

        for px in x_start..x_end {
            let is_left = px < x_mid;

            let is_fg = match ch {
                // Full block
                '\u{2588}' => true,

                // Half blocks
                '\u{2580}' => is_top,       // Upper half block ▀
                '\u{2584}' => !is_top,      // Lower half block ▄
                '\u{258C}' => is_left,      // Left half block ▌
                '\u{2590}' => !is_left,     // Right half block ▐

                // Lower vertical fractions (1/8 to 7/8)
                '\u{2581}' => py >= y_end - (h * 1 + 4) / 8,
                '\u{2582}' => py >= y_end - (h * 2 + 4) / 8,
                '\u{2583}' => py >= y_end - (h * 3 + 4) / 8,
                '\u{2585}' => py >= y_end - (h * 5 + 4) / 8,
                '\u{2586}' => py >= y_end - (h * 6 + 4) / 8,
                '\u{2587}' => py >= y_end - (h * 7 + 4) / 8,

                // Upper fraction (1/8)
                '\u{2594}' => py < y_start + (h * 1 + 4) / 8,

                // Left horizontal fractions (1/8 to 7/8)
                '\u{258F}' => px < x_start + (w * 1 + 4) / 8,
                '\u{258E}' => px < x_start + (w * 2 + 4) / 8,
                '\u{258D}' => px < x_start + (w * 3 + 4) / 8,
                '\u{258B}' => px < x_start + (w * 5 + 4) / 8,
                '\u{258A}' => px < x_start + (w * 6 + 4) / 8,
                '\u{2589}' => px < x_start + (w * 7 + 4) / 8,

                // Right fraction (1/8)
                '\u{2595}' => px >= x_end - (w * 1 + 4) / 8,

                // Shades
                '\u{2591}' => (px % 2 == 0) && (py % 2 == 0),            // 25% light shade ░
                '\u{2592}' => (px + py) % 2 == 0,                          // 50% medium shade ▒
                '\u{2593}' => !((px % 2 == 0) && (py % 2 == 0)),          // 75% dark shade ▓

                // Quadrants
                '\u{2596}' => !is_top && is_left,                          // ▖ BL
                '\u{2597}' => !is_top && !is_left,                         // ▗ BR
                '\u{2598}' => is_top && is_left,                           // ▘ TL
                '\u{2599}' => (is_top && is_left) || !is_top,              // ▙ TL, BL, BR
                '\u{259A}' => (is_top && is_left) || (!is_top && !is_left),// ▚ TL, BR
                '\u{259B}' => is_top || (!is_top && is_left),              // ▛ TL, TR, BL
                '\u{259C}' => is_top || (!is_top && !is_left),             // ▜ TL, TR, BR
                '\u{259D}' => is_top && !is_left,                          // ▝ TR
                '\u{259E}' => (is_top && !is_left) || (!is_top && is_left),// ▞ TR, BL
                '\u{259F}' => (is_top && !is_left) || !is_top,             // ▟ TR, BL, BR

                _ => false,
            };

            buffer[row_offset + px] = if is_fg { fg_u32 } else { bg_u32 };
        }
    }
}


// Alpha-blending glyph onto 0xAARRGGBB buffer
#[allow(dead_code)]
fn blend_glyph(
    buffer: &mut [u32],
    screen_width: usize,
    screen_height: usize,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: Color,
) {
    if x < 0 || y < 0 {
        return;
    }
    let ux = x as usize;
    let uy = y as usize;
    let uw = w as usize;
    let uh = h as usize;

    let cr = color.r() as u32;
    let cg = color.g() as u32;
    let cb = color.b() as u32;
    let ca = color.a() as u32;

    for row in 0..uh {
        let py = uy + row;
        if py >= screen_height {
            break;
        }
        for col in 0..uw {
            let px = ux + col;
            if px >= screen_width {
                break;
            }

            let idx = py * screen_width + px;
            let dst = buffer[idx];

            let dr = (dst >> 16) & 0xFF;
            let dg = (dst >> 8) & 0xFF;
            let db = dst & 0xFF;

            let alpha = ca;
            let inv_alpha = 255 - alpha;

            let out_r = (cr * alpha + dr * inv_alpha) / 255;
            let out_g = (cg * alpha + dg * inv_alpha) / 255;
            let out_b = (cb * alpha + db * inv_alpha) / 255;

            buffer[idx] = 0xFF000000 | (out_r << 16) | (out_g << 8) | out_b;
        }
    }
}

fn compute_grid_size(width: u32, height: u32, char_w: f32, line_h: f32) -> (u16, u16) {
    let avail_w = (width as f32 - (PAD_X * 2.0)).max(100.0);
    let avail_h = (height as f32 - (PAD_Y * 2.0)).max(100.0);

    let rows = (avail_h / line_h).floor() as u16;
    let cols = (avail_w / char_w).floor() as u16;

    (rows.clamp(10, 200), cols.clamp(40, 300))
}

fn get_ctrl_byte(physical_key: PhysicalKey, logical_key: &Key) -> Option<u8> {
    // 1. Hardware scancode: 100% layout-independent (works identical in Persian, English, etc.)
    if let PhysicalKey::Code(code) = physical_key {
        let byte = match code {
            KeyCode::KeyA => Some(1),
            KeyCode::KeyB => Some(2),
            KeyCode::KeyC => Some(3),
            KeyCode::KeyD => Some(4),
            KeyCode::KeyE => Some(5),
            KeyCode::KeyF => Some(6),
            KeyCode::KeyG => Some(7),
            KeyCode::KeyH => Some(8),
            KeyCode::KeyI => Some(9),
            KeyCode::KeyJ => Some(10),
            KeyCode::KeyK => Some(11),
            KeyCode::KeyL => Some(12),
            KeyCode::KeyM => Some(13),
            KeyCode::KeyN => Some(14),
            KeyCode::KeyO => Some(15),
            KeyCode::KeyP => Some(16),
            KeyCode::KeyQ => Some(17),
            KeyCode::KeyR => Some(18),
            KeyCode::KeyS => Some(19),
            KeyCode::KeyT => Some(20),
            KeyCode::KeyU => Some(21),
            KeyCode::KeyV => Some(22),
            KeyCode::KeyW => Some(23),
            KeyCode::KeyX => Some(24),
            KeyCode::KeyY => Some(25),
            KeyCode::KeyZ => Some(26),
            KeyCode::BracketLeft => Some(0x1b),  // Escape
            KeyCode::Backslash => Some(0x1c),    // SIGQUIT
            KeyCode::BracketRight => Some(0x1d),
            _ => None,
        };
        if byte.is_some() {
            return byte;
        }
    }

    // 2. Logical character fallback for Persian keyboard layouts
    match logical_key {
        Key::Character(s) => {
            let ch = s.chars().next()?;
            if ch.is_ascii_alphabetic() {
                Some((ch.to_ascii_uppercase() as u8) - b'@')
            } else {
                match ch {
                    'ش' => Some(1),          // a
                    'ذ' => Some(1),          // a alternative
                    'ل' => Some(7),          // g (or b)
                    'ز' | 'ژ' => Some(3),    // c
                    'ی' | 'ئ' => Some(4),    // d
                    'ث' => Some(5),          // e
                    'ب' => Some(6),          // f
                    'ا' | 'آ' => Some(8),    // h
                    'ه' => Some(9),          // i
                    'ت' => Some(10),         // j
                    'ن' => Some(11),         // k
                    'م' => Some(12),         // l
                    'پ' => Some(13),         // m
                    'د' => Some(14),         // n
                    'خ' => Some(15),         // o
                    'ح' => Some(16),         // p
                    'ض' => Some(17),         // q
                    'ق' => Some(18),         // r
                    'س' => Some(19),         // s
                    'ف' => Some(20),         // t
                    'ع' => Some(21),         // u
                    'ر' => Some(22),         // v
                    'ص' => Some(23),         // w
                    'ط' => Some(24),         // x
                    'غ' => Some(25),         // y
                    'ظ' => Some(26),         // z
                    _ => None,
                }
            }
        }
        _ => None,
    }
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuRenderer>,
    state: TerminalState,
}

impl App {
    fn process_key(
        &mut self,
        physical_key: PhysicalKey,
        logical_key: Key,
        text: Option<String>,
    ) -> bool {
        self.state.start_time = Instant::now();
        self.state.last_cursor_visible = true;

        // 1. Modifier keys alone (Control, Shift, Alt, Super, etc.) must NEVER snap scroll or trigger actions
        let is_modifier = matches!(
            logical_key,
            Key::Named(
                NamedKey::Control
                    | NamedKey::Shift
                    | NamedKey::Alt
                    | NamedKey::AltGraph
                    | NamedKey::Super
                    | NamedKey::Meta
                    | NamedKey::Hyper
                    | NamedKey::Fn
                    | NamedKey::FnLock
                    | NamedKey::CapsLock
                    | NamedKey::NumLock
                    | NamedKey::ScrollLock
            )
        ) || matches!(
            physical_key,
            PhysicalKey::Code(
                KeyCode::ControlLeft
                    | KeyCode::ControlRight
                    | KeyCode::ShiftLeft
                    | KeyCode::ShiftRight
                    | KeyCode::AltLeft
                    | KeyCode::AltRight
                    | KeyCode::SuperLeft
                    | KeyCode::SuperRight
                    | KeyCode::CapsLock
                    | KeyCode::NumLock
                    | KeyCode::ScrollLock
            )
        );

        if is_modifier {
            return false;
        }

        // Check Ctrl+Shift+C (Copy) and Ctrl+Shift+V (Paste)
        let is_key_c = matches!(physical_key, PhysicalKey::Code(KeyCode::KeyC))
            || match &logical_key {
                Key::Character(s) => s.eq_ignore_ascii_case("c") || s == "ز" || s == "ژ",
                _ => false,
            };
        let is_key_v = matches!(physical_key, PhysicalKey::Code(KeyCode::KeyV))
            || match &logical_key {
                Key::Character(s) => s.eq_ignore_ascii_case("v") || s == "ر",
                _ => false,
            };

        // Copy shortcut: Ctrl+Shift+C always, or Ctrl+C if text is currently selected
        if self.state.modifiers.control_key() && is_key_c {
            let has_selection = !matches!(self.state.selection, SelectionState::None);
            if self.state.modifiers.shift_key() || has_selection {
                let sel = self.state.get_selected_text();
                if !sel.is_empty() {
                    copy_to_clipboard(&sel);
                }
                return false;
            }
        }

        // Paste shortcut: Ctrl+Shift+V or Ctrl+V
        if self.state.modifiers.control_key() && is_key_v {
            if let Some(clip) = paste_from_clipboard() {
                if self.state.running_command.is_some() {
                    let normalized = normalize_persian_digits(&clip);
                    self.state.write_to_pty(normalized.as_bytes());
                } else {
                    self.state.scroll_offset = 0;
                    for c in clip.chars() {
                        if c == '\n' || c == '\r' {
                            break;
                        }
                        if !c.is_control() {
                            self.state.current_input.push(c);
                        }
                    }
                }
            }
            return false;
        }

        // Check Ctrl zoom shortcuts: Ctrl + Plus/Equal, Ctrl + Minus, Ctrl + 0
        if self.state.modifiers.control_key() {
            let is_zoom_in = matches!(
                physical_key,
                PhysicalKey::Code(KeyCode::Equal | KeyCode::NumpadAdd)
            ) || match &logical_key {
                Key::Character(s) => s == "=" || s == "+",
                _ => false,
            } || matches!(text.as_deref(), Some("=") | Some("+"));

            let is_zoom_out = matches!(
                physical_key,
                PhysicalKey::Code(KeyCode::Minus | KeyCode::NumpadSubtract)
            ) || match &logical_key {
                Key::Character(s) => s == "-" || s == "_",
                _ => false,
            } || matches!(text.as_deref(), Some("-") | Some("_"));

            let is_zoom_reset = matches!(
                physical_key,
                PhysicalKey::Code(KeyCode::Digit0 | KeyCode::Numpad0)
            ) || match &logical_key {
                Key::Character(s) => s == "0" || s == ")",
                _ => false,
            } || matches!(text.as_deref(), Some("0"));

            if is_zoom_in {
                self.state.change_font_size(1.0);
                return false;
            } else if is_zoom_out {
                self.state.change_font_size(-1.0);
                return false;
            } else if is_zoom_reset {
                self.state.reset_font_size();
                return false;
            }
        }

        // If a command is actively running, forward keys directly to the PTY
        if let Some(cmd) = &self.state.running_command {
            if let Ok(mut parser) = cmd.parser.lock() {
                parser.screen_mut().set_scrollback(0);
            }
            if self.state.modifiers.control_key() {
                if let Some(ctrl_byte) = get_ctrl_byte(physical_key, &logical_key) {
                    self.state.write_to_pty(&[ctrl_byte]);
                    return false;
                }
            }

            match logical_key {
                Key::Named(NamedKey::Enter) => {
                    self.state.write_to_pty(b"\r");
                }
                Key::Named(NamedKey::Backspace) => {
                    self.state.write_to_pty(b"\x7f");
                }
                Key::Named(NamedKey::Tab) => {
                    self.state.write_to_pty(b"\t");
                }
                Key::Named(NamedKey::Escape) => {
                    self.state.write_to_pty(b"\x1b");
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.state.write_to_pty(b"\x1b[A");
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.state.write_to_pty(b"\x1b[B");
                }
                Key::Named(NamedKey::ArrowRight) => {
                    self.state.write_to_pty(b"\x1b[C");
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    self.state.write_to_pty(b"\x1b[D");
                }
                Key::Named(NamedKey::Home) => {
                    self.state.write_to_pty(b"\x1b[H");
                }
                Key::Named(NamedKey::End) => {
                    self.state.write_to_pty(b"\x1b[F");
                }
                Key::Named(NamedKey::PageUp) => {
                    self.state.write_to_pty(b"\x1b[5~");
                }
                Key::Named(NamedKey::PageDown) => {
                    self.state.write_to_pty(b"\x1b[6~");
                }
                Key::Named(NamedKey::Delete) => {
                    self.state.write_to_pty(b"\x1b[3~");
                }
                Key::Named(NamedKey::Insert) => {
                    self.state.write_to_pty(b"\x1b[2~");
                }
                Key::Named(NamedKey::F1) => {
                    self.state.write_to_pty(b"\x1bOP");
                }
                Key::Named(NamedKey::F2) => {
                    self.state.write_to_pty(b"\x1bOQ");
                }
                Key::Named(NamedKey::F3) => {
                    self.state.write_to_pty(b"\x1bOR");
                }
                Key::Named(NamedKey::F4) => {
                    self.state.write_to_pty(b"\x1bOS");
                }
                Key::Named(NamedKey::F5) => {
                    self.state.write_to_pty(b"\x1b[15~");
                }
                Key::Named(NamedKey::F6) => {
                    self.state.write_to_pty(b"\x1b[17~");
                }
                Key::Named(NamedKey::F7) => {
                    self.state.write_to_pty(b"\x1b[18~");
                }
                Key::Named(NamedKey::F8) => {
                    self.state.write_to_pty(b"\x1b[19~");
                }
                Key::Named(NamedKey::F9) => {
                    self.state.write_to_pty(b"\x1b[20~");
                }
                Key::Named(NamedKey::F10) => {
                    self.state.write_to_pty(b"\x1b[21~");
                }
                Key::Named(NamedKey::F11) => {
                    self.state.write_to_pty(b"\x1b[23~");
                }
                Key::Named(NamedKey::F12) => {
                    self.state.write_to_pty(b"\x1b[24~");
                }
                _ => {
                    if let Some(t) = text {
                        let normalized = normalize_persian_digits(&t);
                        self.state.write_to_pty(normalized.as_bytes());
                    }
                }
            }
            return false;
        }

        // Normal Mode: Persian Line Editor at Prompt
        if self.state.modifiers.control_key() {
            if let Some(ctrl_byte) = get_ctrl_byte(physical_key, &logical_key) {
                match ctrl_byte {
                    3 => {
                        // Ctrl+C: Cancel current input, print ^C, start fresh prompt line
                        self.state.scroll_offset = 0;
                        self.state.selection = SelectionState::None;
                        self.state.history_cursor = None;
                        self.state.saved_current_input.clear();
                        let prompt = self.state.current_prompt();
                        self.state.push_persian_history(
                            format!("{}{}^C", prompt, self.state.current_input),
                            Color::rgb(195, 232, 141),
                        );
                        self.state.current_input.clear();
                    }
                    12 => {
                        // Ctrl+L: Clear screen
                        self.state.scroll_offset = 0;
                        self.state.selection = SelectionState::None;
                        self.state.history.clear();
                    }
                    22 => {
                        // Ctrl+V: Paste from clipboard at prompt
                        if let Some(clip) = paste_from_clipboard() {
                            self.state.scroll_offset = 0;
                            for c in clip.chars() {
                                if c == '\n' || c == '\r' {
                                    break;
                                }
                                if !c.is_control() {
                                    self.state.current_input.push(c);
                                }
                            }
                        }
                    }
                    4 => {
                        // Ctrl+D: Exit on empty line
                        if self.state.current_input.is_empty() {
                            return true;
                        }
                    }
                    21 => {
                        // Ctrl+U: Clear line
                        self.state.current_input.clear();
                    }
                    23 => {
                        // Ctrl+W: Delete last word
                        if let Some(pos) = self.state.current_input.trim_end().rfind(' ') {
                            self.state.current_input.truncate(pos + 1);
                        } else {
                            self.state.current_input.clear();
                        }
                    }
                    _ => {}
                }
                return false;
            }
        }

        match logical_key {
            Key::Named(NamedKey::Backspace) => {
                self.state.scroll_offset = 0;
                self.state.current_input.pop();
            }
            Key::Named(NamedKey::Enter) => {
                self.state.scroll_offset = 0;
                self.state.selection = SelectionState::None;
                let entered = std::mem::take(&mut self.state.current_input);
                self.state.push_cmd_history(&entered);
                let prompt = self.state.current_prompt();
                self.state.push_persian_history(
                    format!("{}{}", prompt, entered),
                    Color::rgb(195, 232, 141),
                );
                self.state.execute_command(&entered);
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.state.scroll_offset = 0;
                self.state.selection = SelectionState::None;
                self.state.history_up();
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.state.scroll_offset = 0;
                self.state.selection = SelectionState::None;
                self.state.history_down();
            }
            Key::Named(NamedKey::PageUp) => {
                let page_lines = (self.state.window_rows as usize).saturating_sub(2).max(1);
                self.state.scroll_offset = self.state.scroll_offset.saturating_add(page_lines);
            }
            Key::Named(NamedKey::PageDown) => {
                let page_lines = (self.state.window_rows as usize).saturating_sub(2).max(1);
                self.state.scroll_offset = self.state.scroll_offset.saturating_sub(page_lines);
            }
            Key::Named(NamedKey::Escape) => {
                return true;
            }
            _ => {
                if !self.state.modifiers.control_key() {
                    if let Some(t) = text {
                        self.state.scroll_offset = 0;
                        for c in t.chars() {
                            if !c.is_control() {
                                self.state.current_input.push(c);
                            }
                        }
                    }
                }
            }
        }
        false
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let win_attrs = Window::default_attributes()
            .with_title("ترمینال استعداد")
            .with_decorations(false)
            .with_transparent(true)
            .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0));

        let win = Arc::new(event_loop.create_window(win_attrs).unwrap());
        win.set_cursor(CursorIcon::Text);

        let size = win.inner_size();
        let w = size.width.max(1);
        let h = size.height.max(1);

        self.state.window_width = size.width;
        self.state.window_height = size.height;
        let (rows, cols) = compute_grid_size(size.width, size.height, self.state.cell_width, self.state.line_height);
        self.state.window_rows = rows;
        self.state.window_cols = cols;

        let gpu = GpuRenderer::new(win.clone(), w, h);

        self.window = Some(win);
        self.gpu = Some(gpu);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::PtyOutput => {
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
            AppEvent::CommandFinished => {
                // Command finished. Extract output lines if it was normal output
                if let Some(cmd) = self.state.running_command.take() {
                    let p = cmd.parser.lock().unwrap();
                    let mut s = p.screen().clone();
                    let (rows, cols) = s.size();
                    if !s.alternate_screen() {
                        s.set_scrollback(usize::MAX);
                        let total_scrollback = s.scrollback();
                        let mut remaining = total_scrollback;

                        // 1. Extract all scrollback rows in chronological order
                        while remaining > 0 {
                            s.set_scrollback(remaining);
                            let chunk = remaining.min(rows as usize);
                            for r in 0..chunk {
                                let spans = extract_row_spans(&s, r as u16, cols);
                                self.state.history.push(HistoryEntry::Monospace { spans });
                            }
                            remaining -= chunk;
                        }

                        // 2. Extract visible screen rows up to the last non-empty row
                        s.set_scrollback(0);
                        let mut screen_rows_spans = Vec::with_capacity(rows as usize);
                        let mut last_non_empty = None;
                        for r in 0..rows {
                            let spans = extract_row_spans(&s, r, cols);
                            if !spans.is_empty() {
                                last_non_empty = Some(r as usize);
                            }
                            screen_rows_spans.push(spans);
                        }

                        if let Some(last_r) = last_non_empty {
                            for spans in screen_rows_spans.into_iter().take(last_r + 1) {
                                self.state.history.push(HistoryEntry::Monospace { spans });
                            }
                        }
                    }
                }
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.state.modifiers = mods.state();
            }
            WindowEvent::Resized(size) => {
                self.state.window_width = size.width;
                self.state.window_height = size.height;
                self.state.recalculate_history_lines();
                let (new_rows, new_cols) = compute_grid_size(size.width, size.height, self.state.cell_width, self.state.line_height);
                self.state.window_rows = new_rows;
                self.state.window_cols = new_cols;
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                if let Some(cmd) = &self.state.running_command {
                    if let Ok(master) = cmd.master.lock() {
                        let _ = master.resize(PtySize {
                            rows: new_rows,
                            cols: new_cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                    if let Ok(mut parser) = cmd.parser.lock() {
                        parser.screen_mut().set_size(new_rows, new_cols);
                    }
                }
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pos = MousePos {
                    x: position.x as f32,
                    y: position.y as f32,
                };
                self.state.mouse_pos = Some(pos);
                if matches!(self.state.selection, SelectionState::Selecting { .. }) {
                    // Smooth edge auto-scrolling when dragging near/beyond boundaries!
                    if pos.y < PAD_Y {
                        self.state.scroll_offset = self.state.scroll_offset.saturating_add(1);
                    } else if pos.y > (self.state.window_height as f32 - PAD_Y) {
                        self.state.scroll_offset = self.state.scroll_offset.saturating_sub(1);
                    }

                    let doc_pos = self.state.screen_to_doc_pos(pos);
                    if let SelectionState::Selecting { ref mut current, .. } = self.state.selection {
                        *current = doc_pos;
                    }

                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                match button {
                    MouseButton::Left => {
                        match state {
                            ElementState::Pressed => {
                                let now = Instant::now();
                                let cur_screen_pos = self.state.mouse_pos.unwrap_or(MousePos { x: 0.0, y: 0.0 });
                                let cur_doc_pos = self.state.screen_to_doc_pos(cur_screen_pos);
                                let dt = now.duration_since(self.state.last_click_time).as_millis();
                                let (dx, dy) = if let Some(last_pos) = self.state.last_click_pos {
                                    ((cur_screen_pos.x - last_pos.x).abs(), (cur_screen_pos.y - last_pos.y).abs())
                                } else {
                                    (100.0, 100.0)
                                };

                                if dt < 450 && dx < 8.0 && dy < 8.0 {
                                    self.state.click_count = (self.state.click_count % 3) + 1;
                                } else {
                                    self.state.click_count = 1;
                                }
                                self.state.last_click_time = now;
                                self.state.last_click_pos = Some(cur_screen_pos);

                                match self.state.click_count {
                                    2 => {
                                        if let Some((start_pos, end_pos)) = self.state.select_word_at(cur_screen_pos) {
                                            self.state.selection = SelectionState::Selecting {
                                                mode: SelectionMode::Word { orig_start: start_pos, orig_end: end_pos },
                                                anchor: cur_doc_pos,
                                                current: cur_doc_pos,
                                            };
                                        } else {
                                            self.state.selection = SelectionState::Selecting {
                                                mode: SelectionMode::Char,
                                                anchor: cur_doc_pos,
                                                current: cur_doc_pos,
                                            };
                                        }
                                    }
                                    3 => {
                                        if let Some((start_pos, end_pos)) = self.state.select_line_at(cur_screen_pos) {
                                            self.state.selection = SelectionState::Selecting {
                                                mode: SelectionMode::Line { orig_start: start_pos, orig_end: end_pos },
                                                anchor: cur_doc_pos,
                                                current: cur_doc_pos,
                                            };
                                        } else {
                                            self.state.selection = SelectionState::Selecting {
                                                mode: SelectionMode::Char,
                                                anchor: cur_doc_pos,
                                                current: cur_doc_pos,
                                            };
                                        }
                                    }
                                    _ => {
                                        self.state.selection = SelectionState::Selecting {
                                            mode: SelectionMode::Char,
                                            anchor: cur_doc_pos,
                                            current: cur_doc_pos,
                                        };
                                    }
                                }
                                if let Some(win) = &self.window {
                                    win.request_redraw();
                                }
                            }
                            ElementState::Released => {
                                match self.state.selection {
                                    SelectionState::Selecting { mode, anchor, current } => {
                                        let dx = (current.x - anchor.x).abs();
                                        let dy = (current.y - anchor.y).abs();
                                        match mode {
                                            SelectionMode::Char => {
                                                if dx > 3.0 || dy > 3.0 {
                                                    self.state.selection = SelectionState::Selected { start: anchor, end: current };
                                                    let text = self.state.get_selected_text();
                                                    if !text.is_empty() {
                                                        copy_to_clipboard(&text);
                                                    }
                                                } else {
                                                    self.state.selection = SelectionState::None;
                                                }
                                            }
                                            SelectionMode::Word { orig_start, orig_end } => {
                                                if dx > 6.0 || dy > 6.0 {
                                                    let (s, e) = self.state.selection.get_points(self.state.line_height).unwrap_or((orig_start, orig_end));
                                                    self.state.selection = SelectionState::Selected { start: s, end: e };
                                                    let text = self.state.get_selected_text();
                                                    if !text.is_empty() {
                                                        copy_to_clipboard(&text);
                                                    }
                                                } else {
                                                    self.state.selection = SelectionState::Selected { start: orig_start, end: orig_end };
                                                }
                                            }
                                            SelectionMode::Line { orig_start, orig_end } => {
                                                if dy > self.state.line_height / 2.0 {
                                                    let (s, e) = self.state.selection.get_points(self.state.line_height).unwrap_or((orig_start, orig_end));
                                                    self.state.selection = SelectionState::Selected { start: s, end: e };
                                                    let text = self.state.get_selected_text();
                                                    if !text.is_empty() {
                                                        copy_to_clipboard(&text);
                                                    }
                                                } else {
                                                    self.state.selection = SelectionState::Selected { start: orig_start, end: orig_end };
                                                }
                                            }
                                        }
                                        if let Some(win) = &self.window {
                                            win.request_redraw();
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    MouseButton::Middle | MouseButton::Right => {
                        if state == ElementState::Pressed {
                            if let Some(clip) = paste_from_clipboard() {
                                if self.state.running_command.is_some() {
                                    let normalized = normalize_persian_digits(&clip);
                                    self.state.write_to_pty(normalized.as_bytes());
                                } else {
                                    self.state.scroll_offset = 0;
                                    for c in clip.chars() {
                                        if c == '\n' || c == '\r' {
                                            break;
                                        }
                                        if !c.is_control() {
                                            self.state.current_input.push(c);
                                        }
                                    }
                                }
                                if let Some(win) = &self.window {
                                    win.request_redraw();
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => {
                        self.state.scroll_pixel_accum += p.y as f32;
                        let line_step = 18.0; // Responsive threshold for smooth touchpad scrolling
                        let lines = (self.state.scroll_pixel_accum / line_step) as i32;
                        if lines != 0 {
                            self.state.scroll_pixel_accum -= (lines as f32) * line_step;
                        }
                        lines
                    }
                };

                if lines == 0 {
                    return;
                }

                if self.state.modifiers.control_key() {
                    if lines > 0 {
                        self.state.change_font_size(1.0);
                    } else if lines < 0 {
                        self.state.change_font_size(-1.0);
                    }
                    if let Some(win) = &self.window {
                        win.request_redraw();
                    }
                    return;
                }

                if let Some(cmd) = &self.state.running_command {
                    let (alt_screen, mouse_mode, mouse_enc) = {
                        if let Ok(parser) = cmd.parser.lock() {
                            let s = parser.screen();
                            (s.alternate_screen(), s.mouse_protocol_mode(), s.mouse_protocol_encoding())
                        } else {
                            (false, vt100::MouseProtocolMode::None, vt100::MouseProtocolEncoding::Default)
                        }
                    };

                    if mouse_mode != vt100::MouseProtocolMode::None {
                        let (row, col) = if let Some(pos) = self.state.mouse_pos {
                            let r = (((pos.y - PAD_Y) / self.state.line_height).floor() as u16).saturating_add(1);
                            let c = (((pos.x - PAD_X) / self.state.cell_width).floor() as u16).saturating_add(1);
                            (r.clamp(1, self.state.window_rows), c.clamp(1, self.state.window_cols))
                        } else {
                            (1, 1)
                        };

                        let btn = if lines > 0 { 64 } else { 65 };
                        let count = lines.abs();
                        for _ in 0..count {
                            if mouse_enc == vt100::MouseProtocolEncoding::Sgr {
                                let seq = format!("\x1b[<{};{};{}M", btn, col, row);
                                self.state.write_to_pty(seq.as_bytes());
                            } else {
                                let cb = (btn + 32) as u8;
                                let cx = (col.min(223) + 32) as u8;
                                let cy = (row.min(223) + 32) as u8;
                                self.state.write_to_pty(&[0x1b, b'[', b'M', cb, cx, cy]);
                            }
                        }
                    } else if alt_screen {
                        let key = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                        let count = lines.abs().min(5);
                        for _ in 0..count {
                            self.state.write_to_pty(key);
                        }
                    } else {
                        if let Ok(mut parser) = cmd.parser.lock() {
                            let screen = parser.screen_mut();
                            let cur = screen.scrollback() as i32;
                            let target = (cur + lines).max(0) as usize;
                            screen.set_scrollback(target);
                        }
                    }
                } else {
                    let mut current_offset = self.state.scroll_offset as i32;
                    current_offset += lines;
                    self.state.scroll_offset = current_offset.max(0) as usize;
                    if matches!(self.state.selection, SelectionState::Selecting { .. }) {
                        if let Some(pos) = self.state.mouse_pos {
                            let doc_pos = self.state.screen_to_doc_pos(pos);
                            if let SelectionState::Selecting { ref mut current, .. } = self.state.selection {
                                *current = doc_pos;
                            }
                        }
                    }
                }
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key,
                        logical_key,
                        text,
                        ..
                    },
                ..
            } => {
                let should_exit = self.process_key(
                    physical_key,
                    logical_key,
                    text.as_ref().map(|s| s.to_string()),
                );
                if should_exit {
                    event_loop.exit();
                }
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(win), Some(gpu)) = (&self.window, &mut self.gpu) {
                    let size = win.inner_size();
                    if size.width == 0 || size.height == 0 {
                        return;
                    }
                    if size.width != gpu.config.width || size.height != gpu.config.height {
                        gpu.resize(size.width, size.height);
                    }

                    let output = match gpu.surface.get_current_texture() {
                        Ok(frame) => frame,
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            gpu.resize(gpu.config.width, gpu.config.height);
                            return;
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            eprintln!("GPU Out of memory");
                            event_loop.exit();
                            return;
                        }
                        Err(wgpu::SurfaceError::Timeout) => {
                            return;
                        }
                        Err(_) => {
                            return;
                        }
                    };

                    let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

                    gpu.begin_frame();

                    let width = size.width as usize;
                    let height = size.height as usize;
                    let metrics = Metrics::new(self.state.font_size, self.state.line_height);

                    // Mode 1: Running interactive command (sl, btop, agy, etc.)
                    if let Some(cmd) = &self.state.running_command {
                        let (screen, cursor_pos, hide_cursor, _is_alt_screen) = {
                            let p = cmd.parser.lock().unwrap();
                            let s = p.screen();
                            (s.clone(), s.cursor_position(), s.hide_cursor(), s.alternate_screen())
                        };

                        let (term_rows, term_cols) = screen.size();
                        let (cursor_row, cursor_col) = cursor_pos;
                        let cell_w = self.state.cell_width;
                        let line_h = self.state.line_height;

                        for r_idx in 0..term_rows {
                            let y_pos = PAD_Y + (r_idx as f32) * line_h;
                            if y_pos + line_h < 0.0 || y_pos >= (height as f32) {
                                continue;
                            }

                            // 1. Draw cell background colors
                            for col in 0..term_cols {
                                if let Some(cell) = screen.cell(r_idx, col) {
                                    let ch = cell.contents().chars().next().unwrap_or(' ');
                                    if is_block_element(ch) {
                                        continue; // Block elements handle their own bg/fg split
                                    }
                                    let (_, bg) = cell_colors(&cell);
                                    if let Some(bg_color) = bg {
                                        let x_start = (PAD_X + (col as f32) * cell_w).round();
                                        let x_end = (PAD_X + ((col + 1) as f32) * cell_w).round();
                                        gpu.push_rect_color(x_start, y_pos, x_end - x_start, line_h, bg_color);
                                    }
                                }
                            }

                            // Persian row rendering (RTL right-aligned with Estedad)
                            if row_has_persian(&screen, r_idx, term_cols)
                                && !row_is_multicolumn(&screen, r_idx, term_cols)
                                && !row_has_block_element(&screen, r_idx, term_cols)
                            {
                                let mut spans: Vec<(String, Color)> = Vec::new();
                                let mut cur_run = String::new();
                                let mut cur_color = Color::rgb(220, 230, 242);

                                for col in 0..term_cols {
                                    if let Some(cell) = screen.cell(r_idx, col) {
                                        if cell.is_wide_continuation() {
                                            continue;
                                        }
                                        let fg = vt_to_cosmic(cell.fgcolor());
                                        if fg != cur_color && !cur_run.is_empty() {
                                            spans.push((std::mem::take(&mut cur_run), cur_color));
                                            cur_color = fg;
                                        }
                                        let content = cell.contents();
                                        cur_run.push_str(if content.is_empty() { " " } else { content });
                                    }
                                }
                                if !cur_run.is_empty() {
                                    spans.push((cur_run, cur_color));
                                }
                                while let Some(last) = spans.last_mut() {
                                    let trimmed = last.0.trim_end_matches(' ');
                                    if trimmed.is_empty() {
                                        spans.pop();
                                    } else {
                                        last.0 = trimmed.to_string();
                                        break;
                                    }
                                }

                                if !spans.is_empty() {
                                    let avail_width = (width as f32 - (PAD_X * 2.0)).max(10.0);
                                    let mut line_buf = Buffer::new(&mut self.state.font_system, metrics);
                                    line_buf.set_size(Some(avail_width), None);

                                    let default_attrs = Attrs::new().family(Family::Name("Estedad"));
                                    let spans_refs: Vec<(&str, Attrs)> = spans
                                        .iter()
                                        .map(|(txt, col)| (txt.as_str(), default_attrs.clone().color(*col)))
                                        .collect();

                                    line_buf.set_rich_text(spans_refs, &default_attrs, Shaping::Advanced, Some(Align::Right));
                                    gpu.draw_buffer(
                                        &mut line_buf,
                                        PAD_X,
                                        y_pos,
                                        Color::rgb(220, 230, 242),
                                        &mut self.state.font_system,
                                        &mut self.state.swash_cache,
                                    );

                                    if r_idx == cursor_row && !hide_cursor {
                                        let elapsed_ms = self.state.start_time.elapsed().as_millis();
                                        if (elapsed_ms / 500) % 2 == 0 {
                                            let mut min_glyph_x = f32::INFINITY;
                                            for run in line_buf.layout_runs() {
                                                for g in run.glyphs.iter() {
                                                    if g.x < min_glyph_x {
                                                        min_glyph_x = g.x;
                                                    }
                                                }
                                            }
                                            let cur_x_pos = if min_glyph_x.is_infinite() {
                                                (width as f32) - PAD_X - 10.0
                                            } else {
                                                PAD_X + min_glyph_x - 6.0
                                            };
                                            let cx = cur_x_pos.max(4.0).min(width as f32 - 10.0);
                                            let cy = y_pos + 4.0;
                                            gpu.push_rect_hex_alpha(cx, cy, 4.0, 22.0, 0x82AAFF, 255);
                                        }
                                    }
                                }
                                continue;
                            }

                            // Monospace row rendering with CaskaydiaCove Nerd Font Mono & Block Elements
                            let mut col = 0;
                            while col < term_cols {
                                if let Some(cell) = screen.cell(r_idx, col) {
                                    if cell.is_wide_continuation() {
                                        col += 1;
                                        continue;
                                    }
                                    let content = cell.contents();
                                    if content.is_empty() || content == " " {
                                        col += 1;
                                        continue;
                                    }

                                    let ch = content.chars().next().unwrap_or(' ');
                                    let (span_fg, span_bg) = cell_colors(&cell);

                                    if is_block_element(ch) {
                                        let x_start = (PAD_X + (col as f32) * cell_w).round();
                                        let x_end = (PAD_X + ((col + 1) as f32) * cell_w).round();
                                        gpu.draw_block_element(
                                            x_start,
                                            x_end,
                                            y_pos,
                                            y_pos + line_h,
                                            ch,
                                            span_fg,
                                            span_bg,
                                        );
                                        col += 1;
                                        continue;
                                    }

                                    let start_col = col;
                                    let span_bold = cell.bold();
                                    let mut span_text = String::new();

                                    while col < term_cols {
                                        if let Some(c_cell) = screen.cell(r_idx, col) {
                                            if c_cell.is_wide_continuation() {
                                                col += 1;
                                                continue;
                                            }
                                            let c_content = c_cell.contents();
                                            if c_content.is_empty() {
                                                break;
                                            }
                                            let c_ch = c_content.chars().next().unwrap_or(' ');
                                            if is_block_element(c_ch) {
                                                break;
                                            }
                                            let (c_fg, _) = cell_colors(&c_cell);
                                            if c_fg != span_fg || c_cell.bold() != span_bold {
                                                break;
                                            }
                                            if c_content == " " {
                                                if col + 1 >= term_cols {
                                                    break;
                                                }
                                                if let Some(next_cell) = screen.cell(r_idx, col + 1) {
                                                    let next_c = next_cell.contents();
                                                    if next_c.is_empty() || next_c == " " {
                                                        break;
                                                    }
                                                }
                                            }
                                            span_text.push_str(if c_content.is_empty() { " " } else { c_content });
                                            col += 1;
                                        } else {
                                            break;
                                        }
                                    }

                                    let trimmed = span_text.trim_end();
                                    if trimmed.is_empty() {
                                        continue;
                                    }

                                    let span_x = PAD_X + (start_col as f32) * cell_w;
                                    let mut span_buf = Buffer::new(&mut self.state.font_system, metrics);
                                    span_buf.set_size(None, None);

                                    let weight = if span_bold { Weight::BOLD } else { Weight::NORMAL };
                                    let attrs = Attrs::new()
                                        .family(Family::Name("CaskaydiaCove Nerd Font Mono"))
                                        .weight(weight)
                                        .color(span_fg);
                                    span_buf.set_text(trimmed, &attrs, Shaping::Advanced, Some(Align::Left));
                                    gpu.draw_buffer(
                                        &mut span_buf,
                                        span_x,
                                        y_pos,
                                        span_fg,
                                        &mut self.state.font_system,
                                        &mut self.state.swash_cache,
                                    );
                                } else {
                                    col += 1;
                                }
                            }

                            if r_idx == cursor_row && !hide_cursor {
                                let cur_x_pos = PAD_X + (cursor_col as f32) * cell_w;
                                let cx = cur_x_pos.min(width as f32 - 10.0);
                                let cy = y_pos + 4.0;
                                gpu.push_rect_hex_alpha(cx, cy, 4.0, 22.0, 0x82AAFF, 255);
                            }
                        }

                        // Render selection highlight overlay if active in Mode 1
                        if let Some((p1, p2)) = self.state.selection.get_points(self.state.line_height) {
                            let r1 = (((p1.y - PAD_Y) / self.state.line_height).floor() as i32).clamp(0, term_rows as i32 - 1);
                            let c1 = (((p1.x - PAD_X) / cell_w).floor() as i32).clamp(0, term_cols as i32 - 1);
                            let r2 = (((p2.y - PAD_Y) / self.state.line_height).floor() as i32).clamp(0, term_rows as i32 - 1);
                            let c2 = (((p2.x - PAD_X) / cell_w).floor() as i32).clamp(0, term_cols as i32 - 1);

                            let ((r_start, c_start), (r_end, c_end)) = if (r1, c1) <= (r2, c2) {
                                ((r1, c1), (r2, c2))
                            } else {
                                ((r2, c2), (r1, c1))
                            };

                            let at_left_or_mid = r_start < r_end && c_end <= (term_cols as i32) / 2;

                            for r in r_start..=r_end {
                                let (col_min, col_max) = if r_start == r_end {
                                    (c_start, c_end)
                                } else if r == r_start {
                                    (c_start, term_cols as i32 - 1)
                                } else if r == r_end {
                                    if at_left_or_mid {
                                        (0, term_cols as i32 - 1)
                                    } else {
                                        (0, c_end)
                                    }
                                } else {
                                    (0, term_cols as i32 - 1)
                                };

                                let sx = PAD_X + (col_min as f32) * cell_w;
                                let sw = ((col_max - col_min + 1) as f32) * cell_w;
                                let sy = PAD_Y + (r as f32) * self.state.line_height;
                                gpu.push_rect_hex_alpha(sx, sy, sw, self.state.line_height, 0x3D59A1, 130);
                            }
                        }

                        gpu.render(&view);
                        output.present();
                        return;
                    }

                    // Mode 2: Pure Native Persian Terminal (Estedad font, RTL, Right-aligned)
                    let avail_width = (width as f32 - (PAD_X * 2.0)).max(10.0);
                    let cell_w = self.state.cell_width;

                    let prompt = self.state.current_prompt();
                    let full_line = format!("{}{}", prompt, self.state.current_input);

                    let mut input_buf = Buffer::new(&mut self.state.font_system, metrics);
                    input_buf.set_size(Some(avail_width), None);
                    input_buf.set_wrap(Wrap::Glyph);

                    let attrs = Attrs::new().family(Family::Name("Estedad"));
                    input_buf.set_text(&full_line, &attrs, Shaping::Advanced, Some(Align::Right));
                    input_buf.shape_until_scroll(&mut self.state.font_system, false);
                    let input_visual_lines = input_buf.layout_runs().count().max(1);
                    self.state.input_visual_lines = input_visual_lines;

                    let total_visual_lines: usize = self.state.history.iter().map(|e| e.visual_lines()).sum::<usize>() + input_visual_lines;
                    let line_spacing = self.state.line_height as usize;
                    let total_content_height = total_visual_lines * line_spacing + 50;
                    let max_scroll_offset_y = if total_content_height > height {
                        (total_content_height - height) as i32
                    } else {
                        0
                    };

                    let max_scroll_lines = (max_scroll_offset_y as f32 / self.state.line_height).ceil() as usize;
                    self.state.scroll_offset = self.state.scroll_offset.min(max_scroll_lines);
                    let scroll_shift = (self.state.scroll_offset as f32 * self.state.line_height) as i32;
                    let scroll_offset_y = (max_scroll_offset_y - scroll_shift).max(0);

                    // Render History Lines
                    let mut y_offset = PAD_Y - scroll_offset_y as f32;
                    for entry in &self.state.history {
                        let entry_lines = entry.visual_lines();
                        let entry_height = (entry_lines as f32) * self.state.line_height;

                        if y_offset + entry_height > 0.0 && y_offset < (height as f32) {
                            match entry {
                                HistoryEntry::Persian { text, color, .. } => {
                                    let mut line_buf = Buffer::new(&mut self.state.font_system, metrics);
                                    line_buf.set_size(Some(avail_width), None);
                                    line_buf.set_wrap(Wrap::Glyph);

                                    let attrs = Attrs::new().family(Family::Name("Estedad"));
                                    line_buf.set_text(text, &attrs, Shaping::Advanced, Some(Align::Right));
                                    gpu.draw_buffer(
                                        &mut line_buf,
                                        PAD_X,
                                        y_offset,
                                        *color,
                                        &mut self.state.font_system,
                                        &mut self.state.swash_cache,
                                    );
                                }
                                HistoryEntry::Monospace { spans } => {
                                    for (start_col, text, span_fg) in spans {
                                        let mut cur_col = *start_col;
                                        let mut text_run = String::new();
                                        let mut run_start_col = cur_col;

                                        for ch in text.chars() {
                                            if is_block_element(ch) {
                                                if !text_run.is_empty() {
                                                    let span_x = PAD_X + (run_start_col as f32) * cell_w;
                                                    let mut span_buf = Buffer::new(&mut self.state.font_system, metrics);
                                                    span_buf.set_size(None, None);
                                                    let attrs = Attrs::new()
                                                        .family(Family::Name("CaskaydiaCove Nerd Font Mono"))
                                                        .color(*span_fg);
                                                    span_buf.set_text(&text_run, &attrs, Shaping::Advanced, Some(Align::Left));
                                                    gpu.draw_buffer(
                                                        &mut span_buf,
                                                        span_x,
                                                        y_offset,
                                                        *span_fg,
                                                        &mut self.state.font_system,
                                                        &mut self.state.swash_cache,
                                                    );
                                                    text_run.clear();
                                                }
                                                let x_start = (PAD_X + (cur_col as f32) * cell_w).round();
                                                let x_end = (PAD_X + ((cur_col + 1) as f32) * cell_w).round();
                                                gpu.draw_block_element(
                                                    x_start,
                                                    x_end,
                                                    y_offset,
                                                    y_offset + self.state.line_height,
                                                    ch,
                                                    *span_fg,
                                                    None,
                                                );
                                                cur_col += 1;
                                                run_start_col = cur_col;
                                            } else {
                                                if text_run.is_empty() {
                                                    run_start_col = cur_col;
                                                }
                                                text_run.push(ch);
                                                cur_col += 1;
                                            }
                                        }

                                        if !text_run.is_empty() {
                                            let span_x = PAD_X + (run_start_col as f32) * cell_w;
                                            let mut span_buf = Buffer::new(&mut self.state.font_system, metrics);
                                            span_buf.set_size(None, None);
                                            let attrs = Attrs::new()
                                                .family(Family::Name("CaskaydiaCove Nerd Font Mono"))
                                                .color(*span_fg);
                                            span_buf.set_text(&text_run, &attrs, Shaping::Advanced, Some(Align::Left));
                                            gpu.draw_buffer(
                                                &mut span_buf,
                                                span_x,
                                                y_offset,
                                                *span_fg,
                                                &mut self.state.font_system,
                                                &mut self.state.swash_cache,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        y_offset += entry_height;
                    }

                    // Current Input Line with Prompt on the RIGHT
                    let input_height = (input_visual_lines as f32) * self.state.line_height;
                    if y_offset + input_height > 0.0 && y_offset < (height as f32) {
                        gpu.draw_buffer(
                            &mut input_buf,
                            PAD_X,
                            y_offset,
                            Color::rgb(255, 255, 255),
                            &mut self.state.font_system,
                            &mut self.state.swash_cache,
                        );
                    }

                    // Neon cursor
                    let runs: Vec<_> = input_buf.layout_runs().collect();
                    if let Some(last_run) = runs.last() {
                        let visual_line_idx = runs.len().saturating_sub(1);
                        let cursor_line_y = y_offset + (visual_line_idx as f32) * self.state.line_height;

                        let mut min_glyph_x = f32::INFINITY;
                        for g in last_run.glyphs.iter() {
                            if g.x < min_glyph_x {
                                min_glyph_x = g.x;
                            }
                        }

                        let elapsed_ms = self.state.start_time.elapsed().as_millis();
                        let cursor_visible = (elapsed_ms / 500) % 2 == 0;
                        if cursor_visible && cursor_line_y + 4.0 >= 0.0 && (cursor_line_y + 4.0) < height as f32 {
                            let cur_visual_x = if min_glyph_x.is_infinite() {
                                (width as f32) - PAD_X - 10.0
                            } else {
                                PAD_X + min_glyph_x - 6.0
                            };

                            let cur_x = cur_visual_x.max(4.0).min(width as f32 - 10.0);
                            let cur_y = cursor_line_y + 4.0;
                            let cur_w = 4.0;
                            let cur_h = (self.state.line_height - 7.0).max(10.0);

                            gpu.push_rect_hex_alpha(cur_x, cur_y, cur_w, cur_h, 0x82AAFF, 255);
                        }
                    }

                    // Selection highlight overlay for Mode 2
                    if let Some((p1, p2)) = self.state.selection.get_points(self.state.line_height) {
                        let l1 = ((p1.y / self.state.line_height).floor() as usize).min(total_visual_lines.saturating_sub(1));
                        let l2 = ((p2.y / self.state.line_height).floor() as usize).min(total_visual_lines.saturating_sub(1));

                        let ((l_start, x_start), (l_end, x_end)) = if l1 < l2 || (l1 == l2 && p1.x <= p2.x) {
                            ((l1, p1.x), (l2, p2.x))
                        } else {
                            ((l2, p2.x), (l1, p1.x))
                        };

                        let mid_x = width as f32 / 2.0;
                        let at_left_or_mid = l_start < l_end && x_end <= mid_x;

                        for l in l_start..=l_end {
                            let line_y = PAD_Y - scroll_offset_y as f32 + (l as f32) * self.state.line_height;
                            if line_y + self.state.line_height < 0.0 || line_y >= (height as f32) {
                                continue;
                            }

                            let (hx, hw) = if l_start == l_end {
                                let left = x_start.min(x_end).max(PAD_X);
                                let right = x_start.max(x_end).min(width as f32 - PAD_X);
                                (left, (right - left).max(0.0))
                            } else if l == l_start {
                                let left = x_start.max(PAD_X);
                                let right = width as f32 - PAD_X;
                                (left, (right - left).max(0.0))
                            } else if l == l_end {
                                if at_left_or_mid {
                                    let left = PAD_X;
                                    let right = width as f32 - PAD_X;
                                    (left, (right - left).max(0.0))
                                } else {
                                    let left = PAD_X;
                                    let right = x_end.min(width as f32 - PAD_X);
                                    (left, (right - left).max(0.0))
                                }
                            } else {
                                let left = PAD_X;
                                let right = width as f32 - PAD_X;
                                (left, (right - left).max(0.0))
                            };

                            if hw > 0.0 {
                                gpu.push_rect_hex_alpha(hx, line_y, hw, self.state.line_height, 0x3D59A1, 130);
                            }
                        }
                    }

                    gpu.render(&view);
                    output.present();
                }
            }
            _ => (),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.running_command.is_some() {
            // During command execution, sleep until PTY output or user input
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            // At the prompt: sleep, waking up ONLY twice a second (every 500ms) for cursor blink
            let elapsed_ms = self.state.start_time.elapsed().as_millis();
            let cursor_visible = (elapsed_ms / 500) % 2 == 0;
            if cursor_visible != self.state.last_cursor_visible {
                self.state.last_cursor_visible = cursor_visible;
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }

            let remainder = (elapsed_ms % 500) as u64;
            let time_to_next_blink = (500 - remainder).max(10);
            event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(time_to_next_blink)));
        }
    }
}

fn main() {
    let event_loop = EventLoop::<AppEvent>::with_user_event().build().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    let proxy = event_loop.create_proxy();
    let state = TerminalState::new(proxy);

    let mut app = App {
        window: None,
        gpu: None,
        state,
    };

    let _ = event_loop.run_app(&mut app);
}

#[derive(Default)]
struct CsiStreamFilter {
    state: CsiFilterState,
}

#[derive(Default, PartialEq, Eq)]
enum CsiFilterState {
    #[default]
    Ground,
    Escape,
    CsiParams,
}

impl CsiStreamFilter {
    fn filter(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            match self.state {
                CsiFilterState::Ground => {
                    if *b == 0x1B {
                        self.state = CsiFilterState::Escape;
                    }
                }
                CsiFilterState::Escape => {
                    if *b == b'[' {
                        self.state = CsiFilterState::CsiParams;
                    } else if *b == 0x1B {
                        self.state = CsiFilterState::Escape;
                    } else {
                        self.state = CsiFilterState::Ground;
                    }
                }
                CsiFilterState::CsiParams => {
                    if *b >= 0x20 && *b <= 0x3F {
                        // ECMA-48 parameter byte (0x30..=0x3F) or intermediate byte (0x20..=0x2F)
                    } else if *b == b'f' {
                        // HVP command: translate to CUP 'H'
                        *b = b'H';
                        self.state = CsiFilterState::Ground;
                    } else if *b == 0x1B {
                        self.state = CsiFilterState::Escape;
                    } else {
                        self.state = CsiFilterState::Ground;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_real_btop_execution() {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }).unwrap();

        let mut cmd = CommandBuilder::new("/usr/bin/btop");
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().unwrap();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(24, 80, 1000)));
        let parser_clone = Arc::clone(&parser);

        let t = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut filter = CsiStreamFilter::default();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 { break; }
                filter.filter(&mut buf[..n]);
                let mut p = parser_clone.lock().unwrap();
                p.process(&buf[..n]);
            }
        });

        std::thread::sleep(Duration::from_millis(1000));
        let _ = child.kill();
        let _ = child.wait();
        let _ = t.join();

        let p = parser.lock().unwrap();
        let screen = p.screen();
        let (rows, cols) = screen.size();
        println!("Real btop rows: {} cols: {}", rows, cols);

        let mut row0 = String::new();
        for r in 0..rows {
            let mut line = String::new();
            for c in 0..cols {
                if let Some(cell) = screen.cell(r, c) {
                    line.push_str(cell.contents());
                }
            }
            if r == 0 {
                row0 = line.clone();
            }
            println!("btop Row {:2}: |{}|", r, line);
        }
        assert!(row0.contains("cpu"), "Row 0 must contain 'cpu'!");
    }

    #[test]
    fn test_real_htop_execution() {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }).unwrap();

        let mut cmd = CommandBuilder::new("/usr/bin/htop");
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().unwrap();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(24, 80, 1000)));
        let parser_clone = Arc::clone(&parser);

        let t = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut filter = CsiStreamFilter::default();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 { break; }
                filter.filter(&mut buf[..n]);
                let mut p = parser_clone.lock().unwrap();
                p.process(&buf[..n]);
            }
        });

        std::thread::sleep(Duration::from_millis(1000));
        let _ = child.kill();
        let _ = child.wait();
        let _ = t.join();

        let p = parser.lock().unwrap();
        let screen = p.screen();
        let (rows, cols) = screen.size();
        println!("Real htop rows: {} cols: {}", rows, cols);

        for r in 0..rows {
            let mut line = String::new();
            for c in 0..cols {
                if let Some(cell) = screen.cell(r, c) {
                    line.push_str(cell.contents());
                }
            }
            if !line.trim().is_empty() {
                println!("htop Row {:2}: |{}|", r, line);
            }
        }
    }

    #[test]
    fn test_caskaydia_glyph_advances() {
        let mut font_system = FontSystem::new();
        let caskaydia_paths = [
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Regular.ttf",
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Bold.ttf",
        ];
        for path in caskaydia_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }
        let cell_w = measure_cell_advance(&mut font_system, DEFAULT_FONT_SIZE);
        println!("Base cell_w: {}", cell_w);

        let test_chars = ['M', 'a', '1', '─', '│', '┌', '┐', '└', '┘', '╭', '╮', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█', '⣾', '⣽'];
        let metrics = Metrics::new(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        for ch in test_chars {
            let mut buf = Buffer::new(&mut font_system, metrics);
            let s = ch.to_string();
            let attrs = Attrs::new().family(Family::Name("CaskaydiaCove Nerd Font Mono"));
            buf.set_text(&s, &attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(&mut font_system, false);
            let mut glyph_w = 0.0;
            for run in buf.layout_runs() {
                for g in run.glyphs.iter() {
                    glyph_w = g.w;
                }
            }
            println!("Glyph '{}' (U+{:04X}) width: {} (cell_w = {})", ch, ch as u32, glyph_w, cell_w);
            assert_eq!(glyph_w, cell_w, "Glyph {} width should equal cell_w", ch);
        }
    }

    #[test]
    fn test_prompt_selection_glyphs() {
        let mut font_system = FontSystem::new();
        let estedad_paths = [
            "/usr/share/fonts/TTF/Estedad-Regular.ttf",
            "/usr/share/fonts/TTF/Estedad-Medium.ttf",
            "/usr/share/fonts/TTF/Estedad-Bold.ttf",
        ];
        for path in estedad_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }

        let metrics = Metrics::new(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        let avail_width = 800.0;
        let mut buf = Buffer::new(&mut font_system, metrics);
        buf.set_size(Some(avail_width), None);

        let full_line = "\u{200F}farzad:~ ◀ ls -la";
        let attrs = Attrs::new().family(Family::Name("Estedad"));
        buf.set_text(full_line, &attrs, Shaping::Advanced, Some(Align::Right));
        buf.shape_until_scroll(&mut font_system, false);

        for run in buf.layout_runs() {
            for g in run.glyphs.iter() {
                let cluster = &full_line[g.start..g.end];
                println!("Glyph '{}' [{}..{}] x={:.1} w={:.1}", cluster, g.start, g.end, g.x, g.w);
            }
        }
    }

    #[test]
    fn test_word_selection_on_prompt() {
        let mut font_system = FontSystem::new();
        let estedad_paths = [
            "/usr/share/fonts/TTF/Estedad-Regular.ttf",
            "/usr/share/fonts/TTF/Estedad-Medium.ttf",
            "/usr/share/fonts/TTF/Estedad-Bold.ttf",
        ];
        for path in estedad_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }

        let metrics = Metrics::new(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        let avail_width = 900.0;
        let cell_w = 11.13;
        let prompt_text = "\u{200F}[\u{2066}farzad@cachyos\u{2069}: \u{2066}~\u{2069}] $ ls -la".to_string();
        let line_rep = LineRepresentation::Persian {
            text: prompt_text.clone(),
            sub_line: 0,
        };

        let (text, hitboxes) = get_glyphs_for_line(&mut font_system, metrics, avail_width, cell_w, &line_rep);
        assert!(!hitboxes.is_empty(), "Hitboxes must not be empty");

        // 1. Double click on 'farzad'
        let farzad_glyph = hitboxes.iter().find(|h| {
            let s = &text[h.start..h.end];
            s == "f"
        }).expect("Must find a glyph in 'farzad'");

        let char_indices: Vec<(usize, char)> = text.char_indices().collect();
        let click_idx = char_indices.iter().position(|&(idx, _)| idx >= farzad_glyph.start).unwrap();
        assert!(!is_word_separator(char_indices[click_idx].1));

        let mut start_i = click_idx;
        while start_i > 0 && !is_word_separator(char_indices[start_i - 1].1) {
            start_i -= 1;
        }
        let mut end_i = click_idx;
        while end_i + 1 < char_indices.len() && !is_word_separator(char_indices[end_i + 1].1) {
            end_i += 1;
        }

        let byte_start = char_indices[start_i].0;
        let byte_end = char_indices[end_i].0 + char_indices[end_i].1.len_utf8();
        let word = clean_bidi_text(&text[byte_start..byte_end]);
        assert_eq!(word, "farzad", "Selected word must be 'farzad'!");

        // 2. Double click on '~'
        let tilde_glyph = hitboxes.iter().find(|h| {
            &text[h.start..h.end] == "~"
        }).expect("Must find glyph for '~'");

        let click_tilde = char_indices.iter().position(|&(idx, _)| idx >= tilde_glyph.start).unwrap();
        assert!(!is_word_separator(char_indices[click_tilde].1));

        let mut start_t = click_tilde;
        while start_t > 0 && !is_word_separator(char_indices[start_t - 1].1) {
            start_t -= 1;
        }
        let mut end_t = click_tilde;
        while end_t + 1 < char_indices.len() && !is_word_separator(char_indices[end_t + 1].1) {
            end_t += 1;
        }

        let byte_start_t = char_indices[start_t].0;
        let byte_end_t = char_indices[end_t].0 + char_indices[end_t].1.len_utf8();
        let word_tilde = clean_bidi_text(&text[byte_start_t..byte_end_t]);
        assert_eq!(word_tilde, "~", "Selected word must be '~'!");

        // 3. Range extraction over 'farzad'
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        for h in &hitboxes {
            if h.end > byte_start && h.start < byte_end {
                min_x = min_x.min(h.x1);
                max_x = max_x.max(h.x2);
            }
        }
        assert!(min_x.is_finite() && max_x.is_finite());

        // Range query with [min_x, max_x]
        let mut q_min_byte = usize::MAX;
        let mut q_max_byte = 0;
        for h in &hitboxes {
            let center = (h.x1 + h.x2) / 2.0;
            if h.x2 > h.x1 && center >= min_x && center <= max_x {
                q_min_byte = q_min_byte.min(h.start);
                q_max_byte = q_max_byte.max(h.end);
            }
        }
        let extracted = clean_bidi_text(&text[q_min_byte..q_max_byte]);
        assert_eq!(extracted, "farzad", "Extracted range must equal 'farzad'!");

        // 4. Double click on 'cachyos'
        let cachyos_glyph = hitboxes.iter().find(|h| {
            &text[h.start..h.end] == "y"
        }).expect("Must find glyph 'y' in cachyos");

        let click_c = char_indices.iter().position(|&(idx, _)| idx >= cachyos_glyph.start).unwrap();
        let mut start_c = click_c;
        while start_c > 0 && !is_word_separator(char_indices[start_c - 1].1) {
            start_c -= 1;
        }
        let mut end_c = click_c;
        while end_c + 1 < char_indices.len() && !is_word_separator(char_indices[end_c + 1].1) {
            end_c += 1;
        }
        let word_cachyos = clean_bidi_text(&text[char_indices[start_c].0..char_indices[end_c].0 + char_indices[end_c].1.len_utf8()]);
        assert_eq!(word_cachyos, "cachyos", "Selected word must be 'cachyos'!");

        // 5. Double click on 'ls'
        let ls_glyph = hitboxes.iter().find(|h| {
            &text[h.start..h.end] == "l" && h.start == 37
        }).expect("Must find glyph 'l' in command 'ls'");

        let click_ls = char_indices.iter().position(|&(idx, _)| idx >= ls_glyph.start).unwrap();
        let mut start_ls = click_ls;
        while start_ls > 0 && !is_word_separator(char_indices[start_ls - 1].1) {
            start_ls -= 1;
        }
        let mut end_ls = click_ls;
        while end_ls + 1 < char_indices.len() && !is_word_separator(char_indices[end_ls + 1].1) {
            end_ls += 1;
        }
        let word_ls = clean_bidi_text(&text[char_indices[start_ls].0..char_indices[end_ls].0 + char_indices[end_ls].1.len_utf8()]);
        assert_eq!(word_ls, "ls", "Selected word must be 'ls'!");
    }

    #[test]
    fn test_font_size_metrics_and_grid() {
        let mut font_system = FontSystem::new();
        let caskaydia_paths = [
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Regular.ttf",
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Bold.ttf",
        ];
        for path in caskaydia_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }

        let base_cell_w = measure_cell_advance(&mut font_system, DEFAULT_FONT_SIZE);
        let zoomed_in_w = measure_cell_advance(&mut font_system, DEFAULT_FONT_SIZE + 2.0);
        let zoomed_out_w = measure_cell_advance(&mut font_system, DEFAULT_FONT_SIZE - 5.0);

        assert!(zoomed_in_w > base_cell_w, "Zoomed in cell width should be larger than base");
        assert!(zoomed_out_w < base_cell_w, "Zoomed out cell width should be smaller than base");

        let (rows_base, cols_base) = compute_grid_size(1024, 768, base_cell_w, DEFAULT_LINE_HEIGHT);
        let (rows_zoomed, cols_zoomed) = compute_grid_size(1024, 768, zoomed_in_w, (21.0 * (DEFAULT_LINE_HEIGHT / DEFAULT_FONT_SIZE)).round());

        assert!(rows_zoomed <= rows_base);
        assert!(cols_zoomed <= cols_base);

        let clamped_min = (-50.0f32).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        let clamped_max = (150.0f32).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        assert_eq!(clamped_min, MIN_FONT_SIZE);
        assert_eq!(clamped_max, MAX_FONT_SIZE);
    }

    #[test]
    fn test_inspect_agy_screen() {
        if !std::path::Path::new("/tmp/agy_signedin.raw").exists() {
            return;
        }
        let mut data = std::fs::read("/tmp/agy_signedin.raw").unwrap();
        let mut filter = CsiStreamFilter::default();
        filter.filter(&mut data);
        let mut parser = vt100::Parser::new(28, 90, 1000);
        parser.process(&data);
        let screen = parser.screen();
        let mut font_system = FontSystem::new();
        let caskaydia_paths = [
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Regular.ttf",
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Bold.ttf",
        ];
        for path in caskaydia_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }
        let metrics = Metrics::new(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        let cell_w = measure_cell_advance(&mut font_system, DEFAULT_FONT_SIZE);
        println!("cell_w: {}", cell_w);

        for r in 1..=5 {
            println!("--- Row {} ---", r);
            for c in 0..16 {
                if let Some(cell) = screen.cell(r, c) {
                    let ch = cell.contents();
                    let ch_display = if ch.is_empty() || ch == " " { "_" } else { ch };
                    let (fg, bg) = cell_colors(&cell);
                    println!("  c={:2}: ch='{}', fg={:?}, bg={:?}", c, ch_display, fg, bg);
                }
            }
        }

        let width = 800usize;
        let height = 300usize;
        let mut buffer = vec![0xFF0F111Au32; width * height];
        let mut swash_cache = SwashCache::new();

        let (term_rows, term_cols) = screen.size();
        let line_h_usize = DEFAULT_LINE_HEIGHT as usize;

        for r_idx in 0..term_rows {
            let y_pos = PAD_Y as i32 + (r_idx as i32) * (DEFAULT_LINE_HEIGHT as i32);
            if y_pos + (DEFAULT_LINE_HEIGHT as i32) < 0 || y_pos >= (height as i32) {
                continue;
            }
            let y_start = y_pos as usize;
            let y_end = (y_pos + line_h_usize as i32) as usize;

            // 1. Cell backgrounds for non-block cells
            for col in 0..term_cols {
                if let Some(cell) = screen.cell(r_idx, col) {
                    let ch = cell.contents().chars().next().unwrap_or(' ');
                    if is_block_element(ch) {
                        continue; // Block elements handle their own bg/fg split
                    }
                    let (_, bg) = cell_colors(&cell);
                    if let Some(bg_color) = bg {
                        let x_start = (PAD_X + (col as f32) * cell_w).round() as usize;
                        let x_end = (PAD_X + ((col + 1) as f32) * cell_w).round() as usize;
                        let bg_u32 = color_to_u32(bg_color);
                        for py in y_start..y_end.min(height) {
                            for px in x_start..x_end.min(width) {
                                buffer[py * width + px] = bg_u32;
                            }
                        }
                    }
                }
            }

            // 2. Foreground text and block elements
            let mut col = 0;
            while col < term_cols {
                if let Some(cell) = screen.cell(r_idx, col) {
                    if cell.is_wide_continuation() {
                        col += 1;
                        continue;
                    }
                    let content = cell.contents();
                    if content.is_empty() || content == " " {
                        col += 1;
                        continue;
                    }

                    let ch = content.chars().next().unwrap_or(' ');
                    let (span_fg, span_bg) = cell_colors(&cell);

                    // If it is a block element, draw it with pixel-perfect geometry!
                    if is_block_element(ch) {
                        let x_start = (PAD_X + (col as f32) * cell_w).round() as usize;
                        let x_end = (PAD_X + ((col + 1) as f32) * cell_w).round() as usize;
                        draw_block_element(
                            &mut buffer,
                            width,
                            height,
                            x_start,
                            x_end,
                            y_start,
                            y_end,
                            ch,
                            span_fg,
                            span_bg,
                        );
                        col += 1;
                        continue;
                    }

                    let start_col = col;
                    let span_bold = cell.bold();
                    let mut span_text = String::new();

                    while col < term_cols {
                        if let Some(c_cell) = screen.cell(r_idx, col) {
                            if c_cell.is_wide_continuation() {
                                col += 1;
                                continue;
                            }
                            let c_content = c_cell.contents();
                            if c_content.is_empty() || c_content == " " {
                                break;
                            }
                            let c_ch = c_content.chars().next().unwrap_or(' ');
                            if is_block_element(c_ch) {
                                break;
                            }
                            let (c_fg, _) = cell_colors(&c_cell);
                            if c_fg != span_fg || c_cell.bold() != span_bold {
                                break;
                            }
                            span_text.push_str(c_content);
                            col += 1;
                        } else {
                            break;
                        }
                    }

                    let span_x = PAD_X + (start_col as f32) * cell_w;
                    let weight = if span_bold { Weight::BOLD } else { Weight::NORMAL };
                    let mut span_buf = Buffer::new(&mut font_system, metrics);
                    span_buf.set_size(None, None);
                    let attrs = Attrs::new()
                        .family(Family::Name("CaskaydiaCove Nerd Font Mono"))
                        .weight(weight)
                        .color(span_fg);
                    span_buf.set_text(&span_text, &attrs, Shaping::Advanced, Some(Align::Left));
                    span_buf.shape_until_scroll(&mut font_system, false);

                    span_buf.draw(
                        &mut font_system,
                        &mut swash_cache,
                        span_fg,
                        |gx, gy, gw, gh, color| {
                            blend_glyph(
                                &mut buffer,
                                width,
                                height,
                                gx + (span_x as i32),
                                gy + y_pos,
                                gw,
                                gh,
                                color,
                            );
                        },
                    );
                } else {
                    col += 1;
                }
            }
        }

        // Save raw bytes
        let raw_bytes: Vec<u8> = buffer.iter().flat_map(|p| p.to_ne_bytes()).collect();
        std::fs::write("/tmp/rendered_agy_pixels.raw", raw_bytes).unwrap();
    }

    #[test]
    fn test_mode2_selection_survives_scrolling() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = EventLoop::<AppEvent>::with_user_event();
        builder.with_any_thread(true);
        let event_loop = match builder.build() {
            Ok(el) => el,
            Err(_) => return,
        };
        let proxy = event_loop.create_proxy();
        let mut state = TerminalState::new(proxy);

        let estedad_paths = [
            "/usr/share/fonts/TTF/Estedad-Regular.ttf",
            "/usr/share/fonts/TTF/Estedad-Medium.ttf",
            "/usr/share/fonts/TTF/Estedad-Bold.ttf",
        ];
        for path in estedad_paths {
            if std::path::Path::new(path).exists() {
                let _ = state.font_system.db_mut().load_font_file(path);
            }
        }

        for i in 0..15 {
            state.push_persian_history(format!("History Line {}", i), Color::rgb(255, 255, 255));
        }

        let doc_y_5 = 5.0 * state.line_height + state.line_height / 2.0;
        let p1 = MousePos { x: 0.0, y: doc_y_5 };
        let p2 = MousePos { x: 1024.0, y: doc_y_5 };

        state.selection = SelectionState::Selected { start: p1, end: p2 };

        state.scroll_offset = 0;
        let text_at_0 = extract_mode2_selection(&mut state, p1, p2);
        assert!(text_at_0.contains("History Line 5"), "Selection must extract Line 5 at scroll 0, got: '{}'", text_at_0);

        state.scroll_offset = 4;
        let text_at_4 = extract_mode2_selection(&mut state, p1, p2);
        assert_eq!(text_at_0, text_at_4, "Selection extraction must NOT change when scrolling!");

        state.scroll_offset = 10;
        let text_at_10 = extract_mode2_selection(&mut state, p1, p2);
        assert_eq!(text_at_0, text_at_10, "Selection extraction must be invariant to scrolling!");
    }

    #[test]
    fn test_scroll_invariant_to_ctrl_and_copy() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = EventLoop::<AppEvent>::with_user_event();
        builder.with_any_thread(true);
        let event_loop = match builder.build() {
            Ok(el) => el,
            Err(_) => return,
        };
        let proxy = event_loop.create_proxy();
        let mut state = TerminalState::new(proxy);

        for i in 0..20 {
            state.push_persian_history(format!("History Line {}", i), Color::rgb(255, 255, 255));
        }

        state.scroll_offset = 7;
        let p1 = MousePos { x: 0.0, y: 100.0 };
        let p2 = MousePos { x: 300.0, y: 100.0 };
        state.selection = SelectionState::Selected { start: p1, end: p2 };

        let mut app = App {
            window: None,
            gpu: None,
            state,
        };

        // 1. Simulate pressing Left Control key alone
        app.state.modifiers = ModifiersState::CONTROL;
        app.process_key(
            PhysicalKey::Code(KeyCode::ControlLeft),
            Key::Named(NamedKey::Control),
            None,
        );

        assert_eq!(app.state.scroll_offset, 7, "scroll_offset must NOT reset when pressing Control key!");

        // 2. Simulate pressing C while Control is held (Ctrl+C with active selection)
        app.process_key(
            PhysicalKey::Code(KeyCode::KeyC),
            Key::Character("c".into()),
            Some("c".into()),
        );

        assert_eq!(app.state.scroll_offset, 7, "scroll_offset must NOT reset when copying with Ctrl+C!");

        // 3. Simulate pressing Shift+Control+C (Ctrl+Shift+C)
        app.state.modifiers = ModifiersState::CONTROL | ModifiersState::SHIFT;
        app.process_key(
            PhysicalKey::Code(KeyCode::KeyC),
            Key::Character("C".into()),
            Some("C".into()),
        );

        assert_eq!(app.state.scroll_offset, 7, "scroll_offset must NOT reset when copying with Ctrl+Shift+C!");
    }

    #[test]
    fn test_ls_multicolumn_with_persian() {
        let mut font_system = FontSystem::new();
        let font_paths = [
            "/usr/share/fonts/TTF/Estedad-Regular.ttf",
            "/usr/share/fonts/TTF/CaskaydiaCoveNerdFontMono-Regular.ttf",
        ];
        for path in font_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }

        let rows = 20u16;
        let cols = 150u16;
        let mut parser = vt100::Parser::new(rows, cols, 2000);

        let line1 = "acpi_patch               finalserver_local                PortProton                         ssdt5.dat\r\n";
        let line2 = "facp.dat                 my-baios.bin                     ssdt11.dat                        'شناسنامه اثر شهرزاد بدری.docx'\r\n";
        let line3 = "facs.dat                 nvidia-bug-report.log.gz         ssdt12.dat                        'فارس-محور برنامه نویسی-برنامه های کاریردی-خاص-بدری'\r\n";

        parser.process(line1.as_bytes());
        parser.process(line2.as_bytes());
        parser.process(line3.as_bytes());

        let screen = parser.screen();
        let width = 1800usize;
        let height = 300usize;
        let metrics = Metrics::new(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        let cell_w = measure_cell_advance(&mut font_system, DEFAULT_FONT_SIZE);
        let avail_width = (width as f32 - (PAD_X * 2.0)).max(10.0);

        // Case A: Current behavior (HistoryEntry::Persian with Align::Right)
        let mut buffer_persian = vec![0xFF0F111Au32; width * height];
        let mut swash_cache = SwashCache::new();
        for r in 0..3 {
            let row_str = screen.rows(0, cols).nth(r).unwrap();
            let trimmed = row_str.trim_end();
            let text = format!("\u{200F}\u{2066}{}\u{2069}", trimmed);
            let mut line_buf = Buffer::new(&mut font_system, metrics);
            line_buf.set_size(Some(avail_width), None);
            line_buf.set_wrap(Wrap::Glyph);
            let attrs = Attrs::new().family(Family::Name("Estedad"));
            line_buf.set_text(&text, &attrs, Shaping::Advanced, Some(Align::Right));
            line_buf.shape_until_scroll(&mut font_system, false);
            let y_offset = PAD_Y as i32 + (r as i32) * (DEFAULT_LINE_HEIGHT as i32);
            line_buf.draw(
                &mut font_system,
                &mut swash_cache,
                Color::rgb(220, 230, 242),
                |gx, gy, gw, gh, col| {
                    blend_glyph(&mut buffer_persian, width, height, gx + (PAD_X as i32), gy + y_offset, gw, gh, col);
                },
            );
        }
        let raw_bytes: Vec<u8> = buffer_persian.iter().flat_map(|p| p.to_ne_bytes()).collect();
        std::fs::write("/tmp/test_ls_broken.raw", raw_bytes).unwrap();

        // Case B: Monospace behavior with column segmentation (>= 2 spaces or color change)
        let mut buffer_mono = vec![0xFF0F111Au32; width * height];
        for r in 0..3 {
            let y_offset = PAD_Y as i32 + (r as i32) * (DEFAULT_LINE_HEIGHT as i32);
            let mut col = 0;
            while col < cols {
                if let Some(cell) = screen.cell(r as u16, col) {
                    let content = cell.contents();
                    if content.is_empty() || content == " " {
                        col += 1;
                        continue;
                    }
                    let start_col = col;
                    let span_fg = vt_to_cosmic(cell.fgcolor());
                    let mut text = String::new();
                    while col < cols {
                        if let Some(c_cell) = screen.cell(r as u16, col) {
                            let c_content = c_cell.contents();
                            let c_fg = vt_to_cosmic(c_cell.fgcolor());
                            if c_fg != span_fg {
                                break;
                            }
                            if c_content == " " {
                                // check if next cell is also space (column boundary!)
                                if col + 1 < cols {
                                    if let Some(next_cell) = screen.cell(r as u16, col + 1) {
                                        let next_c = next_cell.contents();
                                        if next_c.is_empty() || next_c == " " {
                                            break;
                                        }
                                    }
                                }
                            }
                            text.push_str(if c_content.is_empty() { " " } else { c_content });
                            col += 1;
                        } else {
                            break;
                        }
                    }
                    let trimmed_text = text.trim_end();
                    if trimmed_text.is_empty() {
                        continue;
                    }
                    let span_x = PAD_X + (start_col as f32) * cell_w;
                    let mut span_buf = Buffer::new(&mut font_system, metrics);
                    span_buf.set_size(None, None);
                    let attrs = Attrs::new().family(Family::Name("CaskaydiaCove Nerd Font Mono")).color(span_fg);
                    span_buf.set_text(trimmed_text, &attrs, Shaping::Advanced, Some(Align::Left));
                    span_buf.shape_until_scroll(&mut font_system, false);
                    span_buf.draw(
                        &mut font_system,
                        &mut swash_cache,
                        span_fg,
                        |gx, gy, gw, gh, col| {
                            blend_glyph(&mut buffer_mono, width, height, gx + (span_x as i32), gy + y_offset, gw, gh, col);
                        },
                    );
                } else {
                    col += 1;
                }
            }
        }
        let raw_bytes: Vec<u8> = buffer_mono.iter().flat_map(|p| p.to_ne_bytes()).collect();
        std::fs::write("/tmp/test_ls_segmented.raw", raw_bytes).unwrap();

        let spans_row1 = extract_row_spans(&screen, 1, cols);
        assert_eq!(spans_row1.len(), 4, "Row 1 should have exactly 4 columns");
        assert_eq!(spans_row1[0].0, 0, "Col 0 start");
        assert_eq!(spans_row1[0].1, "facp.dat");
        assert_eq!(spans_row1[1].0, 25, "Col 1 start");
        assert_eq!(spans_row1[1].1, "my-baios.bin");
        assert_eq!(spans_row1[2].0, 58, "Col 2 start");
        assert_eq!(spans_row1[2].1, "ssdt11.dat");
        assert_eq!(spans_row1[3].0, 92, "Col 3 start");
        assert_eq!(spans_row1[3].1, "'شناسنامه اثر شهرزاد بدری.docx'");

        let spans_row2 = extract_row_spans(&screen, 2, cols);
        assert_eq!(spans_row2.len(), 4, "Row 2 should have exactly 4 columns");
        assert_eq!(spans_row2[0].0, 0);
        assert_eq!(spans_row2[0].1, "facs.dat");
        assert_eq!(spans_row2[1].0, 25);
        assert_eq!(spans_row2[1].1, "nvidia-bug-report.log.gz");
        assert_eq!(spans_row2[2].0, 58);
        assert_eq!(spans_row2[2].1, "ssdt12.dat");
        assert_eq!(spans_row2[3].0, 92);
        assert_eq!(spans_row2[3].1, "'فارس-محور برنامه نویسی-برنامه های کاریردی-خاص-بدری'");
    }

    #[test]
    fn test_input_line_wrapping() {
        let mut font_system = FontSystem::new();
        let estedad_paths = [
            "/usr/share/fonts/TTF/Estedad-Regular.ttf",
            "/usr/share/fonts/TTF/Estedad-Medium.ttf",
            "/usr/share/fonts/TTF/Estedad-Bold.ttf",
        ];
        for path in estedad_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }

        let width = 800usize;
        let height = 200usize;
        let avail_width = (width as f32 - (PAD_X * 2.0)).max(10.0);
        let metrics = Metrics::new(DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        let mut input_buf = Buffer::new(&mut font_system, metrics);
        input_buf.set_size(Some(avail_width), None);
        input_buf.set_wrap(cosmic_text::Wrap::Glyph);

        let prompt = "\u{200F}[\u{2066}farzad@cachyos\u{2069}: \u{2066}~\u{2069}] $ ";
        // Test typing English command that wraps
        let sample_text = "cargo test -- --nocapture test_inspect_agy_screen_with_very_long_argument_to_wrap_around_and_keep_typing_more_and_more_characters_to_test_multi_line_wrapping_in_terminal";
        for i in [5, 30, 45, 60, 75, 80, 150] {
            let input = &sample_text[..i.min(sample_text.len())];
            let full_line = format!("{}{}", prompt, input);
            let attrs = Attrs::new().family(Family::Name("Estedad"));
            input_buf.set_text(&full_line, &attrs, Shaping::Advanced, Some(Align::Right));
            input_buf.shape_until_scroll(&mut font_system, false);

            let runs: Vec<_> = input_buf.layout_runs().collect();
            println!("=== English input len {} ({} lines) ===", i, runs.len());
            for (r_idx, run) in runs.iter().enumerate() {
                let mut glyph_text = String::new();
                let min_x = run.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
                let max_x = run.glyphs.iter().map(|g| g.x + g.w).fold(f32::NEG_INFINITY, f32::max);
                for g in run.glyphs.iter() {
                    glyph_text.push_str(&full_line[g.start..g.end]);
                }
                println!("  Line {}: line_i={} y={:.1} w={:.1} min_x={:.1} max_x={:.1} glyphs={} text='{}'", r_idx, run.line_i, run.line_y, run.line_w, min_x, max_x, run.glyphs.len(), glyph_text);
                if i == 5 {
                    println!("    Line 0 glyphs for len 5:");
                    for (gi, g) in run.glyphs.iter().enumerate() {
                        let ch = &full_line[g.start..g.end];
                        println!("      glyph {}: ch='{}' x={:.1} w={:.1} start={} end={} is_ltr={}", gi, ch, g.x, g.w, g.start, g.end, g.level.is_ltr());
                    }
                }
                if r_idx == 1 {
                    for (gi, g) in run.glyphs.iter().enumerate() {
                        let ch = &full_line[g.start..g.end];
                        println!("    glyph {}: ch='{}' x={:.1} w={:.1} start={} end={}", gi, ch, g.x, g.w, g.start, g.end);
                    }
                }
            }

            if i >= 75 && i < 150 {
                assert_eq!(runs.len(), 2, "Length {} should wrap to 2 lines", i);
                assert!(runs[0].line_w > 740.0, "Line 0 must remain full (w={:.1}) without word drop", runs[0].line_w);
            } else if i == 150 {
                assert_eq!(runs.len(), 3, "Length 150 should wrap to 3 lines");
                assert!(runs[0].line_w > 740.0, "Line 0 must remain full (w={:.1})", runs[0].line_w);
                assert!(runs[1].line_w > 740.0, "Line 1 must remain full (w={:.1})", runs[1].line_w);
            }

            let mut buffer = vec![0xFF0F111Au32; width * height];
            let mut swash_cache = SwashCache::new();
            input_buf.draw(
                &mut font_system,
                &mut swash_cache,
                Color::rgb(255, 255, 255),
                |gx, gy, gw, gh, color| {
                    blend_glyph(&mut buffer, width, height, gx + (PAD_X as i32), gy + (PAD_Y as i32), gw, gh, color);
                },
            );
            let raw_bytes: Vec<u8> = buffer.iter().flat_map(|p| p.to_ne_bytes()).collect();
            std::fs::write(format!("/tmp/wrap_frame_{}.raw", i), raw_bytes).unwrap();
        }

        // Test typing Persian text that wraps
        let persian_sample = "سلام این یک دستور تستی بسیار طولانی به زبان فارسی است که می‌خواهیم تست کنیم چطور خط می‌شکند و نشانگر کجا قرار می‌گیرد";
        for i in [30, 50, 70] {
            let input: String = persian_sample.chars().take(i).collect();
            let full_line = format!("{}{}", prompt, input);
            let attrs = Attrs::new().family(Family::Name("Estedad"));
            input_buf.set_text(&full_line, &attrs, Shaping::Advanced, Some(Align::Right));
            input_buf.shape_until_scroll(&mut font_system, false);

            let runs: Vec<_> = input_buf.layout_runs().collect();
            println!("=== Persian input len {} chars ({} lines) ===", i, runs.len());
            for (r_idx, run) in runs.iter().enumerate() {
                let mut glyph_text = String::new();
                let min_x = run.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
                let max_x = run.glyphs.iter().map(|g| g.x + g.w).fold(f32::NEG_INFINITY, f32::max);
                for g in run.glyphs.iter() {
                    glyph_text.push_str(&full_line[g.start..g.end]);
                }
                println!("  Line {}: line_i={} y={:.1} w={:.1} min_x={:.1} max_x={:.1} glyphs={} text='{}'", r_idx, run.line_i, run.line_y, run.line_w, min_x, max_x, run.glyphs.len(), glyph_text);
                if r_idx == 1 {
                    for (gi, g) in run.glyphs.iter().enumerate() {
                        let ch = &full_line[g.start..g.end];
                        println!("    fa glyph {}: ch='{}' x={:.1} w={:.1} start={} end={}", gi, ch, g.x, g.w, g.start, g.end);
                    }
                }
            }

            if i == 70 {
                assert_eq!(runs.len(), 2, "Persian input len 70 should wrap to 2 lines");
                assert!(runs[0].line_w > 740.0, "Persian Line 0 must remain full without empty void (w={:.1})", runs[0].line_w);
            }

            let mut buffer = vec![0xFF0F111Au32; width * height];
            let mut swash_cache = SwashCache::new();
            input_buf.draw(
                &mut font_system,
                &mut swash_cache,
                Color::rgb(255, 255, 255),
                |gx, gy, gw, gh, color| {
                    blend_glyph(&mut buffer, width, height, gx + (PAD_X as i32), gy + (PAD_Y as i32), gw, gh, color);
                },
            );
            let raw_bytes: Vec<u8> = buffer.iter().flat_map(|p| p.to_ne_bytes()).collect();
            std::fs::write(format!("/tmp/wrap_frame_fa_{}.raw", i), raw_bytes).unwrap();
        }
    }

    #[test]
    fn test_vt100_scrollback_behavior() {
        let mut parser = vt100::Parser::new(5, 40, 100);
        for i in 0..12 {
            parser.process(format!("line {}\r\n", i).as_bytes());
        }
        let mut s = parser.screen().clone();
        println!("Initial scrollback: {}", s.scrollback());
        s.set_scrollback(usize::MAX);
        let total_sb = s.scrollback();
        println!("Total scrollback after set_scrollback(MAX): {}", total_sb);
        let (rows, cols) = s.size();
        println!("Screen size: {}x{}", rows, cols);

        let mut extracted_lines = Vec::new();
        let mut remaining = total_sb;
        while remaining > 0 {
            s.set_scrollback(remaining);
            let chunk = remaining.min(rows as usize);
            for r in 0..chunk {
                let mut line = String::new();
                for c in 0..cols {
                    if let Some(cell) = s.cell(r as u16, c) {
                        line.push_str(cell.contents());
                    }
                }
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    extracted_lines.push(trimmed.to_string());
                }
            }
            remaining -= chunk;
        }

        s.set_scrollback(0);
        for r in 0..rows {
            let mut line = String::new();
            for c in 0..cols {
                if let Some(cell) = s.cell(r, c) {
                    line.push_str(cell.contents());
                }
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                extracted_lines.push(trimmed.to_string());
            }
        }

        println!("Extracted lines: {:?}", extracted_lines);
        let expected: Vec<String> = (0..12).map(|i| format!("line {}", i)).collect();
        assert_eq!(extracted_lines, expected);
    }

    #[test]
    fn test_agy_persian_vs_ls_multicolumn() {
        let cols = 100u16;
        let mut parser = vt100::Parser::new(10, cols, 100);

        // Row 0: agy Persian typing line
        parser.process(b"> \xD8\xB3\xD9\x84\xD8\xA7\xD9\x85 \xD8\xA7\xDB\x8C\xD9\x86 \xDB\x8C\xDA\xA9 \xD8\xAA\xD8\xB3\xD8\xAA \xD8\xA7\xD8\xB3\xD8\xAA\r\n");
        // Row 1: agy box-drawing divider
        parser.process("\u{2500}".repeat(cols as usize).as_bytes());
        parser.process(b"\r\n");
        // Row 2: ls multicolumn line with Persian
        parser.process(b"file1.txt                '\xD9\x81\xD8\xA7\xDB\x8C\xD9\x84 \xD8\xAA\xD8\xB3\xD8\xAA\xDB\x8C.txt'                test.rs\r\n");
        // Row 3: English input line
        parser.process(b"> hello world\r\n");

        let screen = parser.screen().clone();

        // 1. agy Persian typing line: must be detected as Persian prose, NOT multicolumn, NOT block element
        assert!(row_has_persian(&screen, 0, cols), "Row 0 should have Persian");
        assert!(!row_has_block_element(&screen, 0, cols), "Row 0 should not have block elements");
        assert!(!row_is_multicolumn(&screen, 0, cols), "Row 0 should NOT be multicolumn");

        // 2. agy divider: must be detected as block element
        assert!(row_has_block_element(&screen, 1, cols), "Row 1 should have block elements");

        // 3. ls line: must be detected as multicolumn
        assert!(row_has_persian(&screen, 2, cols), "Row 2 should have Persian");
        assert!(row_is_multicolumn(&screen, 2, cols), "Row 2 must be multicolumn");

        // 4. English line: no Persian
        assert!(!row_has_persian(&screen, 3, cols), "Row 3 should not have Persian");

        // 5. Test cosmic-text shaping of Row 0 with Align::Right
        let mut font_system = FontSystem::new();
        let estedad_paths = [
            "/usr/share/fonts/TTF/Estedad-Regular.ttf",
            "/usr/share/fonts/TTF/Estedad-Medium.ttf",
            "/usr/share/fonts/TTF/Estedad-Bold.ttf",
        ];
        for path in estedad_paths {
            if std::path::Path::new(path).exists() {
                let _ = font_system.db_mut().load_font_file(path);
            }
        }
        let metrics = Metrics::new(16.0, 24.0);
        let avail_width = 800.0f32;
        let mut line_buf = Buffer::new(&mut font_system, metrics);
        line_buf.set_size(Some(avail_width), None);
        let text = "> سلام این یک تست است";
        let default_attrs = Attrs::new().family(Family::Name("Estedad"));
        line_buf.set_text(text, &default_attrs, Shaping::Advanced, Some(Align::Right));
        line_buf.shape_until_scroll(&mut font_system, false);

        let runs: Vec<_> = line_buf.layout_runs().collect();
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        println!("run line_w: {:.1}, glyphs: {}", run.line_w, run.glyphs.len());
        let min_x = run.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
        let max_x = run.glyphs.iter().map(|g| g.x + g.w).fold(f32::NEG_INFINITY, f32::max);
        println!("min_x: {:.1}, max_x: {:.1}", min_x, max_x);
        assert!(max_x > avail_width - 10.0, "Right-aligned text should reach the right margin! max_x={:.1}", max_x);
    }

    #[test]
    fn test_bottom_left_corner_selection() {
        // 1. Test Mode 1 (interactive / command output)
        let parser = Arc::new(Mutex::new(vt100::Parser::new(5, 80, 100)));
        {
            let mut p = parser.lock().unwrap();
            p.process(b"Line 0 - first line\r\n");
            p.process(b"Line 1 - second line\r\n");
            p.process(b"Line 2 - third line with some text on the right side\r\n");
        }
        let cell_w = 11.0f32;
        let line_h = 24.0f32;

        // User drags from top (Line 0, col 0) to bottom-left corner of Line 2 (x = PAD_X + 5.0, near left corner)
        let p1 = MousePos { x: PAD_X, y: PAD_Y + 5.0 };
        let p2 = MousePos { x: PAD_X + 5.0, y: PAD_Y + 2.0 * line_h + 5.0 };

        let selected = extract_mode1_selection(&parser, cell_w, line_h, p1, p2);
        println!("Mode 1 extracted text with bottom-left drag:\n{}", selected);
        assert!(selected.contains("Line 0 - first line"));
        assert!(selected.contains("Line 1 - second line"));
        assert!(selected.contains("Line 2 - third line with some text on the right side"), "Line 2 must be fully selected when dragging to bottom-left corner!");

        // User stops at col 25 (<= term_cols / 2 = 40): even up to the middle, it completes the line!
        let p2_mid = MousePos { x: PAD_X + 25.0 * cell_w, y: PAD_Y + 2.0 * line_h + 5.0 };
        let selected_mid = extract_mode1_selection(&parser, cell_w, line_h, p1, p2_mid);
        assert!(selected_mid.contains("on the right side"), "Drag up to the middle must complete the line!");

        // User stops past the middle at col 60 (> 40): fine-grained selection on right half
        let p2_past_mid = MousePos { x: PAD_X + 45.0 * cell_w, y: PAD_Y + 2.0 * line_h + 5.0 };
        let selected_past_mid = extract_mode1_selection(&parser, cell_w, line_h, p1, p2_past_mid);
        assert!(!selected_past_mid.contains("on the right side"), "Drag past the middle should not auto-complete the rest");

        // 2. Test Mode 2 (Persian terminal history / prompt)
        use winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = EventLoop::<AppEvent>::with_user_event();
        builder.with_any_thread(true);
        if let Ok(event_loop) = builder.build() {
            let proxy = event_loop.create_proxy();
            let mut state = TerminalState::new(proxy);
            state.push_persian_history("اولین خط تاریخچه ترمینال".to_string(), Color::rgb(255, 255, 255));
            state.push_persian_history("دومین خط تست متن فارسی".to_string(), Color::rgb(255, 255, 255));
            state.push_persian_history("سومین خط در گوشه سمت راست قرار دارد".to_string(), Color::rgb(255, 255, 255));

            let p1 = MousePos { x: 750.0, y: 10.0 };
            // Drag to x = 350.0 (<= mid_x = 400.0, up to the middle of the screen)
            let p2 = MousePos { x: 350.0, y: 2.0 * state.line_height + 10.0 };

            let selected = extract_mode2_selection(&mut state, p1, p2);
            println!("Mode 2 extracted text with drag up to the middle:\n{}", selected);
            assert!(selected.contains("اولین خط"));
            assert!(selected.contains("دومین خط"));
            assert!(selected.contains("سومین خط در گوشه سمت راست قرار دارد"), "Mode 2 Line 2 must be fully completed when dragging up to the middle!");
        }
    }

    #[test]
    fn test_command_history_navigation() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = EventLoop::<AppEvent>::with_user_event();
        builder.with_any_thread(true);
        if let Ok(event_loop) = builder.build() {
            let proxy = event_loop.create_proxy();
            let mut state = TerminalState::new(proxy);

            // Override cmd_history for clean test isolation
            state.cmd_history = vec![
                "ls -la".to_string(),
                "cargo build".to_string(),
                "git status".to_string(),
            ];
            state.history_cursor = None;
            state.current_input = "carg".to_string();

            // 1. Press ArrowUp -> should remember "carg" and display "git status"
            state.history_up();
            assert_eq!(state.current_input, "git status");
            assert_eq!(state.history_cursor, Some(2));
            assert_eq!(state.saved_current_input, "carg");

            // 2. Press ArrowUp again -> "cargo build"
            state.history_up();
            assert_eq!(state.current_input, "cargo build");
            assert_eq!(state.history_cursor, Some(1));

            // 3. Press ArrowUp again -> "ls -la"
            state.history_up();
            assert_eq!(state.current_input, "ls -la");
            assert_eq!(state.history_cursor, Some(0));

            // 4. Press ArrowUp at oldest -> stays at "ls -la"
            state.history_up();
            assert_eq!(state.current_input, "ls -la");
            assert_eq!(state.history_cursor, Some(0));

            // 5. Press ArrowDown -> "cargo build"
            state.history_down();
            assert_eq!(state.current_input, "cargo build");
            assert_eq!(state.history_cursor, Some(1));

            // 6. Press ArrowDown -> "git status"
            state.history_down();
            assert_eq!(state.current_input, "git status");
            assert_eq!(state.history_cursor, Some(2));

            // 7. Press ArrowDown -> restores "carg"
            state.history_down();
            assert_eq!(state.current_input, "carg");
            assert_eq!(state.history_cursor, None);

            // 8. Press ArrowDown at bottom -> stays "carg"
            state.history_down();
            assert_eq!(state.current_input, "carg");
            assert_eq!(state.history_cursor, None);

            // 9. Execute new command
            state.push_cmd_history("echo hello");
            assert_eq!(state.cmd_history.last().unwrap(), "echo hello");
            assert_eq!(state.history_cursor, None);
        }
    }

    #[test]
    fn test_vulkan_gpu_quad_layout() {
        use crate::gpu::{GpuQuad, Uniforms};

        // Ensure GpuQuad is exactly 48 bytes (12 * 4 bytes f32)
        assert_eq!(std::mem::size_of::<GpuQuad>(), 48);
        // Ensure Uniforms is exactly 16 bytes (WGSL uniform alignment requirement)
        assert_eq!(std::mem::size_of::<Uniforms>(), 16);

        let quad = GpuQuad {
            rect: [10.0, 20.0, 100.0, 200.0],
            uv: [0.0, 0.0, 1.0, 1.0],
            color: [1.0, 0.5, 0.25, 1.0],
        };
        let bytes = bytemuck::bytes_of(&quad);
        assert_eq!(bytes.len(), 48);
    }

    #[test]
    fn test_translate_persian_composite_commands() {
        assert_eq!(translate_persian_command("سودو نانو"), "sudo nano");
        assert_eq!(translate_persian_command("سودو نانو /etc/hosts"), "sudo nano /etc/hosts");
        assert_eq!(translate_persian_command("سودو ال اس"), "sudo ls --color=auto");
        assert_eq!(translate_persian_command("سودو ال اس -la"), "sudo ls --color=auto -la");
        assert_eq!(translate_persian_command("گیت استاتوس"), "git status");
        assert_eq!(translate_persian_command("گیت کامیت -m \"test\""), "git commit -m \"test\"");
        assert_eq!(translate_persian_command("سودو سیستم‌سی‌تی‌ال ریستارت caddy"), "sudo systemctl restart caddy");
        assert_eq!(translate_persian_command("میک دیر /tmp/test"), "mkdir -p /tmp/test");
        assert_eq!(translate_persian_command("سودو پاک"), "sudo clear");
    }

    #[test]
    fn test_translate_persian_keyboard_typos() {
        assert_eq!(translate_persian_command("سعیخ دشدخ"), "sudo nano");
        assert_eq!(translate_persian_command("سعیخ دشدخ /etc/hosts"), "sudo nano /etc/hosts");
        assert_eq!(translate_persian_command("لهف سفشفعس"), "git status");
        assert_eq!(translate_persian_command("مس"), "ls --color=auto");
        assert_eq!(translate_persian_command("مس -مش"), "ls --color=auto -la");
        assert_eq!(translate_persian_command("سعیخ مس -مش"), "sudo ls --color=auto -la");
        assert_eq!(translate_persian_command("سعیخ قثذخخف"), "sudo reboot");
        assert_eq!(translate_persian_command("زمثشق"), "clear");
        assert_eq!(translate_persian_command("زشف /etc/os-release"), "cat /etc/os-release");
        assert_eq!(translate_persian_command("رهپ /etc/hosts"), "vim /etc/hosts");
        assert_eq!(translate_persian_command("زی .."), "cd ..");
    }

    #[test]
    fn test_translate_chained_and_piped_commands() {
        assert_eq!(translate_persian_command("سودو نانو && سودو ریبوت"), "sudo nano && sudo reboot");
        assert_eq!(translate_persian_command("سعیخ دشدخ && سعیخ قثذخخف"), "sudo nano && sudo reboot");
        assert_eq!(translate_persian_command("سودو نانو ; سودو ریبوت"), "sudo nano ; sudo reboot");
        assert_eq!(translate_persian_command("سودو نانو ؛ سودو ریبوت"), "sudo nano ; sudo reboot");
        assert_eq!(translate_persian_command("ال اس | grep foo"), "ls --color=auto | grep foo");
    }

    #[test]
    fn test_persian_arguments_and_literals_preserved() {
        assert_eq!(translate_persian_command("echo سلام دنیا"), "echo سلام دنیا");
        assert_eq!(translate_persian_command("ایکو سلام دنیا"), "echo سلام دنیا");
        assert_eq!(translate_persian_command("mkdir سلام"), "mkdir سلام");
        assert_eq!(translate_persian_command("بساز پوشه_جدید"), "mkdir -p پوشه_جدید");
        assert_eq!(translate_persian_command("git commit -m \"تغییرات جدید\""), "git commit -m \"تغییرات جدید\"");
    }
}




