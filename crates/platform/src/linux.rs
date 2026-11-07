//! Linux（X11）平台层：全局键盘钩子、模拟输入、光标/前台窗口、权限。
//!
//! 全局监听优先用 evdev `grab`（可以吞掉快捷键，需要 input 组权限）；
//! 失败时回退到 XRecord `listen`（只监听、不吞键）。模拟输入走 XTest（rdev）或 xdotool。
//! 系统音量 ducking 在 Linux 上暂不实现，返回 None，控制器会安全跳过。

use crate::volume::OutputVolumeSnapshot;
use crate::{HookFn, KeyEvent, KeyKind, MicStatus, Rect};
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 自己注入的事件在 XRecord 回退路径里没有 user-data 标记，用时间窗屏蔽。
static INJECTING: AtomicBool = AtomicBool::new(false);

const INJECT_GUARD: Duration = Duration::from_millis(30);

fn with_injecting<T>(f: impl FnOnce() -> T) -> T {
    INJECTING.store(true, Ordering::SeqCst);
    let r = f();
    std::thread::sleep(INJECT_GUARD);
    INJECTING.store(false, Ordering::SeqCst);
    r
}

fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v == "wayland")
            .unwrap_or(false)
}

/// Wayland 下逐字上屏要反复改剪贴板，改为结束后一次性粘贴。
pub fn live_text_supported() -> bool {
    !wayland()
}

/// 原生 Wayland 文字注入是实验路径：需要 wl-clipboard + ydotool/uinput。
/// 默认走 XWayland/XTest，避免和键盘重映射工具（如 Toshy/XWayKeyz）冲突。
fn wayland_ydotool() -> bool {
    wayland() && std::env::var_os("VOICEKEY_WAYLAND_YDOTOOL").is_some()
}

fn ydotool_socket() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".ydotool_socket")
}

/// 启动 ydotoold（uinput 注入器）。已存在 socket 时直接复用。
fn ensure_ydotoold() -> bool {
    let sock = ydotool_socket();
    if sock.exists() {
        return true;
    }
    let spawned = Command::new("ydotoold")
        .arg("--socket-path")
        .arg(&sock)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok();
    if !spawned {
        return false;
    }
    for _ in 0..20 {
        if sock.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    sock.exists()
}

fn ydotool(args: &[&str]) -> bool {
    Command::new("ydotool")
        .args(args)
        .env("YDOTOOL_SOCKET", ydotool_socket())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn wl_copy(text: &str) -> bool {
    let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            let _ = child.kill();
            return false;
        }
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn wl_paste() -> Option<String> {
    let out = Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Wayland 上屏：优先 wl-copy 写剪贴板，再用 uinput 发 Ctrl+V。
/// 这样原生 Wayland 应用也能收到；不依赖 XTest。
fn wayland_paste(text: &str) {
    let previous = wl_paste();
    let copied = wl_copy(text);
    if !copied {
        // 没有 wl-clipboard 时退回 X11 剪贴板（XWayland 会桥接到 Wayland）。
        let Ok(mut cb) = arboard::Clipboard::new() else {
            eprintln!("Wayland 文字上屏失败：wl-copy 不可用，且无法打开 X11 剪贴板");
            return;
        };
        if cb.set_text(text.to_string()).is_err() {
            eprintln!("Wayland 文字上屏失败：无法写入剪贴板");
            return;
        }
    }
    // 给剪贴板 owner 一点时间就绪，再发 Ctrl+V。
    std::thread::sleep(Duration::from_millis(80));
    if !ensure_ydotoold() {
        eprintln!("Wayland 文字上屏失败：ydotoold 不可用（需要 input 组和 /dev/uinput 权限）");
        return;
    }
    if !ydotool(&[
        "key",
        "-d",
        "10",
        "29:1",
        "47:1",
        "47:0",
        "29:0",
    ]) {
        eprintln!("Wayland 文字上屏失败：ydotool Ctrl+V 执行失败");
        return;
    }
    if let Some(previous) = previous {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            wl_copy(&previous);
        });
    }
}

#[derive(Default)]
struct HookState {
    mods: u8,
    down: HashSet<u32>,
}

impl HookState {
    fn convert(&mut self, ev: &rdev::Event) -> Option<KeyEvent> {
        use rdev::EventType;
        let (pressed, code) = match ev.event_type {
            EventType::KeyPress(k) => (true, key_code(k)),
            EventType::KeyRelease(k) => (false, key_code(k)),
            _ => return None,
        };
        if code == 0 {
            return None;
        }
        let is_mod = keys::modifier(code).is_some();
        let repeat = if pressed {
            !self.down.insert(code)
        } else {
            self.down.remove(&code);
            false
        };
        if let Some((flag, _)) = keys::modifier(code) {
            if pressed {
                self.mods |= flag;
            } else {
                self.mods &= !flag;
            }
        }
        let kind = if is_mod {
            KeyKind::Modifier(pressed)
        } else if pressed {
            KeyKind::Down
        } else {
            KeyKind::Up
        };
        let chars = if pressed && !is_mod {
            ev.name.clone()
        } else {
            None
        };
        Some(KeyEvent {
            kind,
            code,
            mods: self.mods,
            repeat,
            chars,
        })
    }
}

struct Ctx {
    hook: HookFn,
    state: HookState,
}

fn dispatch(ctx: &Mutex<Ctx>, ev: &rdev::Event) -> bool {
    if INJECTING.load(Ordering::SeqCst) {
        return false;
    }
    let Ok(mut c) = ctx.lock() else { return false };
    let Some(ke) = c.state.convert(ev) else {
        return false;
    };
    (c.hook)(&ke)
}

/// 安装全局键盘钩子；不阻塞调用方。
pub fn start_hook(f: HookFn) {
    let ctx = Arc::new(Mutex::new(Ctx {
        hook: f,
        state: HookState::default(),
    }));
    std::thread::spawn(move || {
        // 先尝试 evdev grab（可吞键）；没有 input/uinput 权限时自动回退 XRecord。
        // 运行中热插拔键盘后 rdev 的 grab 可能吞掉新设备事件，可用环境变量强制走 XRecord。
        if std::env::var_os("VOICEKEY_LINUX_NO_GRAB").is_none() {
            let c = ctx.clone();
            match rdev::grab(move |ev| if dispatch(&c, &ev) { None } else { Some(ev) }) {
                Ok(()) => return,
                Err(e) => eprintln!(
                    "evdev 全局钩子不可用（{e:?}），回退 XRecord；如需吞掉快捷键请把用户加入 input 组"
                ),
            }
        }
        let c = ctx;
        if let Err(e) = rdev::listen(move |ev| {
            dispatch(&c, &ev);
        }) {
            eprintln!("全局键盘钩子启动失败（{e:?}）：请确认使用 X11 会话，或把用户加入 input 组");
        }
    });
}

pub fn inject_key(code: u32, down: bool) {
    if code == 0 {
        return;
    }
    with_injecting(|| {
        let k = rdev::Key::Unknown(code);
        let ev = if down {
            rdev::EventType::KeyPress(k)
        } else {
            rdev::EventType::KeyRelease(k)
        };
        let _ = rdev::simulate(&ev);
    });
}

pub(crate) fn backspace(n: usize) {
    if n == 0 {
        return;
    }
    if wayland_ydotool() {
        if ensure_ydotoold() {
            for _ in 0..n {
                let _ = ydotool(&["key", "-d", "5", "14:1", "14:0"]);
            }
        }
        return;
    }
    if with_injecting(|| {
        Command::new("xdotool")
            .args([
                "key",
                "--clearmodifiers",
                "--repeat",
                &n.to_string(),
                "BackSpace",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }) {
        return;
    }
    with_injecting(|| {
        for _ in 0..n {
            let _ = rdev::simulate(&rdev::EventType::KeyPress(rdev::Key::Backspace));
            let _ = rdev::simulate(&rdev::EventType::KeyRelease(rdev::Key::Backspace));
        }
    });
}

fn xdotool_type(text: &str) -> bool {
    Command::new("xdotool")
        .args(["type", "--clearmodifiers", "--delay", "0", "--", text])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ctrl_v() {
    if Command::new("xdotool")
        .args(["key", "--clearmodifiers", "ctrl+v"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return;
    }
    // 没有 xdotool 时用 XTest 直接发 Ctrl+V
    use rdev::{EventType, Key};
    for (k, down) in [
        (Key::ControlLeft, true),
        (Key::KeyV, true),
        (Key::KeyV, false),
        (Key::ControlLeft, false),
    ] {
        let ev = if down {
            EventType::KeyPress(k)
        } else {
            EventType::KeyRelease(k)
        };
        let _ = rdev::simulate(&ev);
    }
}

fn clipboard_type(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if cb.set_text(text.to_string()).is_ok() {
            with_injecting(ctrl_v);
        }
    }
}

pub(crate) fn type_text(text: &str) {
    if text.is_empty() {
        return;
    }
    if wayland_ydotool() {
        wayland_paste(text);
        return;
    }
    if with_injecting(|| xdotool_type(text)) {
        return;
    }
    clipboard_type(text);
}

/// 借剪贴板 + Ctrl+V 上屏，随后恢复原剪贴板文本（图片不处理）。
pub fn paste(text: &str) {
    if wayland_ydotool() {
        wayland_paste(text);
        return;
    }
    let Ok(mut cb) = arboard::Clipboard::new() else {
        type_text(text);
        return;
    };
    let saved = cb.get_text().ok();
    if cb.set_text(text.to_string()).is_err() {
        type_text(text);
        return;
    }
    with_injecting(ctrl_v);
    if saved.is_none() {
        return;
    }
    let text = text.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if cb.get_text().ok().as_deref() == Some(text.as_str()) {
                if let Some(t) = saved {
                    let _ = cb.set_text(t);
                }
            }
        }
    });
}

/// Linux 上取插入符位置需要 AT-SPI/输入法特有协议，先返回 None，
/// 候选框会退回到鼠标所在屏幕的底部居中位置。
pub fn caret() -> Option<Rect> {
    None
}

#[derive(Clone, Copy, Debug)]
pub struct FrontApp(isize);

pub fn front_app() -> Option<FrontApp> {
    // Wayland 下 xdotool 只能看到 XWayland 窗口，拿到的可能不是当前焦点应用。
    if wayland() {
        return None;
    }
    let out = Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let id = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<isize>()
        .ok()?;
    (id != 0).then_some(FrontApp(id))
}

impl FrontApp {
    pub fn activate(&self) {
        let _ = Command::new("xdotool")
            .args(["windowactivate", "--sync", &self.0.to_string()])
            .status();
    }
}

pub fn secure_input() -> bool {
    false
}

pub(crate) fn output_volume_default() -> Option<OutputVolumeSnapshot> {
    None
}

pub(crate) fn output_volume_for(_original: &OutputVolumeSnapshot) -> Option<OutputVolumeSnapshot> {
    None
}

pub(crate) fn output_volume_write(
    _desired: &OutputVolumeSnapshot,
    _expected: &OutputVolumeSnapshot,
) -> Option<OutputVolumeSnapshot> {
    None
}

pub mod perm {
    use super::MicStatus;

    pub fn accessibility(_prompt: bool) -> bool {
        true
    }

    pub fn mic() -> MicStatus {
        MicStatus::Granted
    }

    pub fn request_mic() {}

    pub fn open_accessibility() {}

    pub fn open_mic() {}
}

pub mod keys {
    use crate::{HoldKey, Special, ALT, CTRL, META, SHIFT};

    pub fn modifier(code: u32) -> Option<(u8, &'static str)> {
        Some(match code {
            50 => (SHIFT, "左 Shift"),
            62 => (SHIFT, "右 Shift"),
            37 => (CTRL, "左 Ctrl"),
            105 => (CTRL, "右 Ctrl"),
            64 => (ALT, "左 Alt"),
            108 => (ALT, "右 Alt"),
            133 => (META, "左 Super"),
            134 => (META, "右 Super"),
            _ => return None,
        })
    }

    pub fn special(code: u32) -> Option<Special> {
        Some(match code {
            9 => Special::Escape,
            36 | 104 => Special::Enter,
            111 => Special::Up,
            116 => Special::Down,
            10..=18 => Special::Digit((code - 9) as u8),
            _ => return None,
        })
    }

    /// X11 keycode：F1–F12（F11/F12 不连续）
    pub fn is_function_key(code: u32) -> bool {
        matches!(code, 67..=76 | 95 | 96)
    }

    pub fn special_name(code: u32) -> Option<&'static str> {
        const FK: [u32; 12] = [67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 95, 96];
        const F: [&str; 12] = [
            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        ];
        if let Some(i) = FK.iter().position(|&c| c == code) {
            return Some(F[i]);
        }
        Some(match code {
            65 => "Space",
            36 => "Enter",
            23 => "Tab",
            22 => "Backspace",
            119 => "Delete",
            9 => "Esc",
            113 => "←",
            114 => "→",
            116 => "↓",
            111 => "↑",
            110 => "Home",
            115 => "End",
            112 => "PgUp",
            117 => "PgDn",
            _ => return None,
        })
    }

    pub fn hold_keys() -> &'static [HoldKey] {
        &[
            HoldKey {
                id: "leftAlt",
                code: 64,
                name: "左 Alt",
            },
            HoldKey {
                id: "rightAlt",
                code: 108,
                name: "右 Alt",
            },
            HoldKey {
                id: "leftCtrl",
                code: 37,
                name: "左 Ctrl",
            },
            HoldKey {
                id: "rightCtrl",
                code: 105,
                name: "右 Ctrl",
            },
            HoldKey {
                id: "leftShift",
                code: 50,
                name: "左 Shift",
            },
            HoldKey {
                id: "rightShift",
                code: 62,
                name: "右 Shift",
            },
            HoldKey {
                id: "leftMeta",
                code: 133,
                name: "左 Super",
            },
            HoldKey {
                id: "rightMeta",
                code: 134,
                name: "右 Super",
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ALT, CTRL};

    #[test]
    fn linux_keycodes_match_x11() {
        assert_eq!(keys::modifier(105).map(|(f, _)| f), Some(CTRL));
        assert_eq!(keys::modifier(108).map(|(f, _)| f), Some(ALT));
        assert_eq!(keys::special(10), Some(crate::Special::Digit(1)));
        assert_eq!(keys::special(18), Some(crate::Special::Digit(9)));
        assert_eq!(keys::special(9), Some(crate::Special::Escape));
        assert_eq!(keys::special_name(36), Some("Enter"));
        assert_eq!(keys::special_name(65), Some("Space"));
        assert!(keys::is_function_key(67));
        assert!(keys::is_function_key(96));
        assert!(!keys::is_function_key(97));
    }

    #[test]
    fn rdev_key_maps_to_x11_keycode() {
        assert_eq!(key_code(rdev::Key::ControlLeft), 37);
        assert_eq!(key_code(rdev::Key::ControlRight), 105);
        assert_eq!(key_code(rdev::Key::Alt), 64);
        assert_eq!(key_code(rdev::Key::AltGr), 108);
        assert_eq!(key_code(rdev::Key::ShiftLeft), 50);
        assert_eq!(key_code(rdev::Key::ShiftRight), 62);
        assert_eq!(key_code(rdev::Key::MetaLeft), 133);
        assert_eq!(key_code(rdev::Key::MetaRight), 134);
        assert_eq!(key_code(rdev::Key::KeyV), 55);
        assert_eq!(key_code(rdev::Key::Unknown(1234)), 1234);
    }

    #[test]
    fn hold_keys_are_x11_modifiers() {
        for k in keys::hold_keys() {
            assert!(keys::modifier(k.code).is_some(), "{} 必须是修饰键", k.id);
        }
    }

    #[test]
    fn converts_rdev_events_to_key_events() {
        let mut st = HookState::default();
        let press = |k| rdev::Event {
            time: std::time::SystemTime::now(),
            name: None,
            event_type: rdev::EventType::KeyPress(k),
        };
        let release = |k| rdev::Event {
            time: std::time::SystemTime::now(),
            name: None,
            event_type: rdev::EventType::KeyRelease(k),
        };

        let ctrl = st.convert(&press(rdev::Key::ControlRight)).unwrap();
        assert_eq!(ctrl.kind, KeyKind::Modifier(true));
        assert_eq!(ctrl.code, 105);
        assert_eq!(ctrl.mods, CTRL);

        let mut v = press(rdev::Key::KeyV);
        v.name = Some("v".into());
        let v = st.convert(&v).unwrap();
        assert_eq!(v.kind, KeyKind::Down);
        assert_eq!(v.code, 55);
        assert_eq!(v.chars.as_deref(), Some("v"));
        assert_eq!(v.mods, CTRL);

        let up = st.convert(&release(rdev::Key::KeyV)).unwrap();
        assert_eq!(up.kind, KeyKind::Up);
        let ctrl_up = st.convert(&release(rdev::Key::ControlRight)).unwrap();
        assert_eq!(ctrl_up.mods, 0);
    }
}

/// rdev 的 `Key` 枚举 → X11 keycode（rdev 在 Linux 下使用的就是 X11 keycode）。
fn key_code(k: rdev::Key) -> u32 {
    use rdev::Key as K;
    match k {
        K::Alt => 64,
        K::AltGr => 108,
        K::Backspace => 22,
        K::CapsLock => 66,
        K::ControlLeft => 37,
        K::ControlRight => 105,
        K::Delete => 119,
        K::DownArrow => 116,
        K::End => 115,
        K::Escape => 9,
        K::F1 => 67,
        K::F2 => 68,
        K::F3 => 69,
        K::F4 => 70,
        K::F5 => 71,
        K::F6 => 72,
        K::F7 => 73,
        K::F8 => 74,
        K::F9 => 75,
        K::F10 => 76,
        K::F11 => 95,
        K::F12 => 96,
        K::Home => 110,
        K::LeftArrow => 113,
        K::MetaLeft => 133,
        K::MetaRight => 134,
        K::PageDown => 117,
        K::PageUp => 112,
        K::Return => 36,
        K::RightArrow => 114,
        K::ShiftLeft => 50,
        K::ShiftRight => 62,
        K::Space => 65,
        K::Tab => 23,
        K::UpArrow => 111,
        K::PrintScreen => 107,
        K::ScrollLock => 78,
        K::Pause => 127,
        K::NumLock => 77,
        K::BackQuote => 49,
        K::Num1 => 10,
        K::Num2 => 11,
        K::Num3 => 12,
        K::Num4 => 13,
        K::Num5 => 14,
        K::Num6 => 15,
        K::Num7 => 16,
        K::Num8 => 17,
        K::Num9 => 18,
        K::Num0 => 19,
        K::Minus => 20,
        K::Equal => 21,
        K::KeyQ => 24,
        K::KeyW => 25,
        K::KeyE => 26,
        K::KeyR => 27,
        K::KeyT => 28,
        K::KeyY => 29,
        K::KeyU => 30,
        K::KeyI => 31,
        K::KeyO => 32,
        K::KeyP => 33,
        K::LeftBracket => 34,
        K::RightBracket => 35,
        K::KeyA => 38,
        K::KeyS => 39,
        K::KeyD => 40,
        K::KeyF => 41,
        K::KeyG => 42,
        K::KeyH => 43,
        K::KeyJ => 44,
        K::KeyK => 45,
        K::KeyL => 46,
        K::SemiColon => 47,
        K::Quote => 48,
        K::BackSlash => 51,
        K::IntlBackslash => 94,
        K::KeyZ => 52,
        K::KeyX => 53,
        K::KeyC => 54,
        K::KeyV => 55,
        K::KeyB => 56,
        K::KeyN => 57,
        K::KeyM => 58,
        K::Comma => 59,
        K::Dot => 60,
        K::Slash => 61,
        K::Insert => 118,
        K::KpReturn => 104,
        K::KpMinus => 82,
        K::KpPlus => 86,
        K::KpMultiply => 63,
        K::KpDivide => 106,
        K::Kp0 => 90,
        K::Kp1 => 87,
        K::Kp2 => 88,
        K::Kp3 => 89,
        K::Kp4 => 83,
        K::Kp5 => 84,
        K::Kp6 => 85,
        K::Kp7 => 79,
        K::Kp8 => 80,
        K::Kp9 => 81,
        K::KpDelete => 91,
        K::Function => 0,
        K::Unknown(code) => code,
    }
}
