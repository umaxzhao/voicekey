#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod asr_cli;
mod controller;
mod hotkey;
mod offline;
mod settings;
mod sound;
mod ui;
mod update;
mod wtoffline;

use controller::Msg;
use hotkey::Shared;
use serde_json::json;
use settings::{Channel, Settings};
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::mpsc::{self, UnboundedSender};
use voicekey_core::QwenEngine;
use voicekey_platform::{self as pf, perm, MicStatus};

pub struct AppState {
    pub settings: Arc<RwLock<Settings>>,
    pub hk: Arc<Shared>,
    pub tx: UnboundedSender<Msg>,
    pub qwen: Arc<QwenEngine>,
}

fn apply<R: Runtime>(app: &AppHandle<R>, s: &Settings) {
    let st = app.state::<AppState>();
    {
        let mut c = st.hk.config.write().unwrap();
        c.set_hold(&s.hold_codes());
        c.tap = s.tap_shortcut.clone();
    }
    ui::refresh_tray(app, s, st.hk.paused.load(Ordering::Relaxed));
    let al = app.autolaunch();
    if al.is_enabled().unwrap_or(false) != s.autostart {
        let _ = if s.autostart { al.enable() } else { al.disable() };
    }
}

pub fn update_settings<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&mut Settings)) {
    let s = {
        let st = app.state::<AppState>();
        let mut g = st.settings.write().unwrap();
        f(&mut g);
        g.sanitize();
        g.save();
        g.clone()
    };
    apply(app, &s);
    download_model(app, s.channel);
    let _ = app.emit("settings", &s);
}

/// 本地模型渠道：开始下载（已下载或下载中则忽略）
fn download_model<R: Runtime>(app: &AppHandle<R>, ch: Channel) {
    match ch {
        Channel::Offline => offline::download(app),
        Channel::WetypeOffline => wtoffline::download(app),
        _ => {}
    }
}

pub fn model_ready(ch: Channel) -> bool {
    match ch {
        Channel::Offline => offline::installed(),
        Channel::WetypeOffline => wtoffline::installed(),
        _ => true,
    }
}

pub fn tray_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        "settings" => ui::show_settings(app),
        "update" => update::install(app),
        "quit" => app.exit(0),
        "pause" => {
            let st = app.state::<AppState>();
            let p = !st.hk.paused.load(Ordering::Relaxed);
            st.hk.paused.store(p, Ordering::Relaxed);
            let s = st.settings.read().unwrap().clone();
            ui::refresh_tray(app, &s, p);
        }
        _ => {
            if let Some(ch) = id.strip_prefix("ch:").and_then(|c| serde_json::from_value::<Channel>(json!(c)).ok()) {
                update_settings(app, |s| s.channel = ch);
            }
        }
    }
}

fn send(app: &AppHandle, m: Msg) {
    let _ = app.state::<AppState>().tx.send(m);
}

fn perms() -> serde_json::Value {
    let mic = match perm::mic() {
        MicStatus::Granted => "granted",
        MicStatus::Denied => "denied",
        MicStatus::Undetermined => "undetermined",
    };
    json!({ "accessibility": perm::accessibility(false), "mic": mic })
}

#[tauri::command]
fn get_state(app: AppHandle) -> serde_json::Value {
    let s = app.state::<AppState>().settings.read().unwrap().clone();
    let hold: Vec<_> = pf::hold_keys().iter().map(|k| json!({ "id": k.id, "name": k.name })).collect();
    json!({
        "settings": s,
        "platform": if cfg!(target_os = "macos") {
            "mac"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else {
            "win"
        },
        "arch": std::env::consts::ARCH,
        "liveText": pf::live_text_supported(),
        "version": app.package_info().version.to_string(),
        "holdKeys": hold,
        "channels": Channel::available(),
        "offline": { "supported": offline::SUPPORTED },
        "models": { "offline": offline::status(), "wetypeoffline": wtoffline::status() },
        "perms": perms(),
        "update": update::status(),
    })
}

#[tauri::command]
fn set_settings(app: AppHandle, mut settings: Settings) {
    // 先归一化外部输入，持久化值和发给音量 worker 的值必须完全一致。
    settings.sanitize();
    let new_ducking = settings.lower_system_volume;
    let new_ducking_percent = settings.lower_system_volume_percent;
    let (mic_changed, ducking_changed) = {
        let state = app.state::<AppState>();
        let current = state.settings.read().unwrap();
        (
            current.mic != settings.mic,
            current.lower_system_volume != new_ducking
                || current.lower_system_volume_percent != new_ducking_percent,
        )
    };
    update_settings(&app, |s| *s = settings);
    if mic_changed {
        send(&app, Msg::RefreshMeter);
    }
    if ducking_changed {
        // 开关或比例在录音中修改都立即生效；非录音状态只更新下次使用的参数。
        send(&app, Msg::VolumeDucking { enabled: new_ducking, reduction_percent: new_ducking_percent });
    }
}

#[tauri::command]
fn perm_status() -> serde_json::Value {
    perms()
}

#[tauri::command]
fn perm_action(kind: String) {
    match kind.as_str() {
        "accessibility" => {
            perm::accessibility(true);
            perm::open_accessibility();
        }
        "mic-request" => perm::request_mic(),
        "mic" => perm::open_mic(),
        _ => {}
    }
}

#[tauri::command]
fn model_download(app: AppHandle, ch: Channel) {
    download_model(&app, ch);
}

#[tauri::command]
fn microphones() -> Vec<String> {
    voicekey_core::audio::microphones()
}

#[tauri::command]
fn record_shortcut(app: AppHandle, on: bool, hold: Option<bool>) {
    let hk = &app.state::<AppState>().hk;
    hk.record_hold.store(hold.unwrap_or(false), Ordering::Relaxed);
    hk.recording.store(on, Ordering::Relaxed);
}

#[tauri::command]
fn pick_choose(app: AppHandle, index: usize) {
    send(&app, Msg::PickChoose(index));
}

#[tauri::command]
fn pick_key(app: AppHandle, key: String) {
    use voicekey_platform::Special;
    let k = match key.as_str() {
        "ArrowUp" => Special::Up,
        "ArrowDown" => Special::Down,
        "Enter" => Special::Enter,
        "Escape" => Special::Escape,
        d => match d.parse::<u8>() {
            Ok(n @ 1..=9) => Special::Digit(n),
            _ => return,
        },
    };
    send(&app, Msg::PickKey(k));
}

#[tauri::command]
fn pick_resize(app: AppHandle, height: f64) {
    ui::pick_resize(&app, height);
}

#[tauri::command]
fn compare_toggle(app: AppHandle) {
    send(&app, Msg::CompareToggle);
}

#[tauri::command]
fn meter(app: AppHandle, on: bool) {
    send(&app, Msg::Meter(on));
}

#[tauri::command]
fn preview_sound(app: AppHandle, id: String) {
    crate::sound::play_id(&app, &id);
}

#[tauri::command]
fn list_sounds() -> Vec<serde_json::Value> {
    // 只列出当前平台能发声的音效：非 macOS 无兜底 WAV 的系统音不提供（review #3）
    crate::sound::selectable_sounds().iter().map(|id| json!({ "id": id, "name": id })).collect()
}

#[tauri::command]
fn open_url(url: String) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "linux") {
        "xdg-open"
    } else {
        "explorer"
    };
    let _ = std::process::Command::new(cmd).arg(url).spawn();
}

#[derive(serde::Deserialize)]
struct TermInput {
    canonical: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    hotword: bool,
    #[serde(default)]
    protect: bool,
}

fn terms_json(terms: &[voicekey_core::terms::Term]) -> Vec<serde_json::Value> {
    terms
        .iter()
        .map(|term| {
            json!({
                "canonical": term.canonical,
                "aliases": term.aliases,
                "hotword": term.hotword,
                "protect": term.protect,
            })
        })
        .collect()
}

#[tauri::command]
fn get_terms() -> serde_json::Value {
    let path = voicekey_core::terms::default_path();
    let terms = voicekey_core::terms::Terms::load(&path);
    json!({
        "path": path.display().to_string(),
        "savePath": voicekey_core::terms::terms_path().display().to_string(),
        "terms": terms_json(terms.terms()),
    })
}

#[tauri::command]
fn term_presets() -> Vec<serde_json::Value> {
    terms_json(&voicekey_core::terms::presets())
}

#[tauri::command]
fn save_terms(terms: Vec<TermInput>) -> Result<(), String> {
    let list = terms
        .into_iter()
        .filter_map(|term| {
            let canonical = term.canonical.trim();
            if canonical.is_empty() {
                return None;
            }
            let aliases: Vec<String> = term
                .aliases
                .into_iter()
                .map(|alias| alias.trim().to_string())
                .filter(|alias| !alias.is_empty() && alias != canonical)
                .collect();
            Some(voicekey_core::terms::Term {
                canonical: canonical.to_string(),
                aliases,
                hotword: term.hotword,
                protect: term.protect,
            })
        })
        .collect();
    voicekey_core::terms::Terms::from_terms(list)
        .save(&voicekey_core::terms::terms_path())
        .map_err(|e| e.to_string())
}

fn fcitx5_config_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from(".config"));
    base.join("voicekey/fcitx5.conf")
}

#[tauri::command]
fn get_fcitx5_settings() -> serde_json::Value {
    let path = fcitx5_config_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut hold = "F5".to_string();
    let mut tail = 600u32;
    let mut command = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "holdkey" => hold = value.trim().to_string(),
            "tailms" => tail = value.trim().parse().unwrap_or(600),
            "command" => command = value.trim().to_string(),
            _ => {}
        }
    }
    json!({
        "holdKey": hold,
        "tailMs": tail,
        "command": command,
        "configPath": path.display().to_string(),
    })
}

#[tauri::command]
fn set_fcitx5_settings(hold_key: String, tail_ms: u32) -> Result<(), String> {
    const HOLDS: &[&str] = &[
        "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Alt_R", "Control_R",
        "Shift_R", "Super_L", "Super_R",
    ];
    if !HOLDS.contains(&hold_key.as_str()) {
        return Err("不支持的快捷键".into());
    }
    let path = fcitx5_config_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut has_hold = false;
    let mut has_tail = false;
    let mut has_section = false;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            has_section = true;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("holdkey") {
            lines.push(format!("HoldKey={hold_key}"));
            has_hold = true;
        } else if lower.starts_with("tailms") {
            lines.push(format!("TailMs={}", tail_ms.min(2000)));
            has_tail = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !has_section {
        lines.insert(0, "[VoiceKey]".into());
    }
    if !has_hold {
        lines.push(format!("HoldKey={hold_key}"));
    }
    if !has_tail {
        lines.push(format!("TailMs={}", tail_ms.min(2000)));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("conf.tmp");
    std::fs::write(&tmp, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn restart_fcitx5() -> Result<(), String> {
    let mut command = std::process::Command::new("fcitx5");
    command.args(["-r", "-d"]);
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut addon_dirs = Vec::new();
    if let Some(home) = &home {
        let local = home.join(".local/lib/fcitx5");
        if local.is_dir() {
            addon_dirs.push(local.display().to_string());
        }
    }
    addon_dirs.push("/usr/lib/x86_64-linux-gnu/fcitx5".into());
    command.env("FCITX_ADDON_DIRS", addon_dirs.join(":"));
    let mut data_dirs = Vec::new();
    if let Some(home) = &home {
        let local = home.join(".local/share");
        if local.is_dir() {
            data_dirs.push(local.display().to_string());
        }
    }
    data_dirs.push("/usr/share".into());
    command.env("FCITX_DATA_DIRS", data_dirs.join(":"));
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_check(app: AppHandle) {
    update::check(&app, true);
}

#[tauri::command]
fn update_install(app: AppHandle) {
    update::install(&app);
}

/// 设置页「测试」按钮：用固定的两条示例转写真打一次 Jev，返回选中项与脱敏 key
#[tauri::command]
async fn jev_test(config: voicekey_core::jev::Config) -> Result<voicekey_core::jev::Tested, String> {
    voicekey_core::jev::test(&config).await.map_err(|e| e.to_string())
}

fn main() {
    if let Some(code) = offline::worker_main() {
        std::process::exit(code);
    }
    if let Some(code) = asr_cli::main() {
        std::process::exit(code);
    }
    let settings = Settings::load();
    let first_run = !settings.onboarded;
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| ui::show_settings(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_state, set_settings, perm_status, perm_action, microphones, record_shortcut,
            pick_choose, pick_key, pick_resize, compare_toggle, meter, open_url, model_download,
            update_check, update_install, jev_test, preview_sound, list_sounds,
            get_fcitx5_settings, set_fcitx5_settings, restart_fcitx5,
            get_terms, term_presets, save_terms
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let (tx, rx) = mpsc::unbounded_channel();
            let qwen = Arc::new(QwenEngine::default());
            let hk = Arc::new(Shared::default());
            app.manage(AppState { settings: Arc::new(RwLock::new(settings.clone())), hk: hk.clone(), tx: tx.clone(), qwen: qwen.clone() });

            let (htx, mut hrx) = mpsc::unbounded_channel();
            hotkey::start(hk, htx);
            tauri::async_runtime::spawn(async move {
                while let Some(e) = hrx.recv().await {
                    let _ = tx.send(Msg::Hot(e));
                }
            });
            let handle = app.handle().clone();
            controller::spawn(handle.clone(), qwen, rx);
            let audio_tx = handle.state::<AppState>().tx.clone();
            voicekey_core::audio::watch_microphones(move |mics| {
                let _ = audio_tx.send(Msg::MicrophonesChanged(mics));
            });
            ui::create_overlays(&handle)?;
            #[cfg(target_os = "linux")]
            if let Err(e) = ui::build_tray(&handle, &settings) {
                // 某些精简桌面没有 StatusNotifier/AppIndicator 宿主，托盘失败不应阻止应用启动。
                eprintln!("托盘初始化失败（已跳过）：{e}");
            }
            #[cfg(not(target_os = "linux"))]
            ui::build_tray(&handle, &settings)?;
            apply(&handle, &settings);
            #[cfg(target_os = "linux")]
            {
                // Linux 桌面上从菜单启动时直接展示设置窗口；托盘应用仍会常驻。
                let _ = first_run;
                ui::show_settings(&handle);
            }
            #[cfg(not(target_os = "linux"))]
            if first_run || !perm::accessibility(false) {
                ui::show_settings(&handle);
            }
            update::schedule(&handle);
            Ok(())
        })
        .on_window_event(|w, e| {
            // 先禁用电平表，再结束 Compare，避免回到 Idle 时 refresh_meter 瞬间重开麦。
            if w.label() == "main" && matches!(e, WindowEvent::Destroyed | WindowEvent::CloseRequested { .. }) {
                let st = w.state::<AppState>();
                let _ = st.tx.send(Msg::Meter(false));
                let _ = st.tx.send(Msg::CompareStop);
                st.hk.recording.store(false, Ordering::Relaxed);
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build app")
        .run(|_app, e| match e {
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => ui::show_settings(_app),
            RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
            _ => {}
        });
}
