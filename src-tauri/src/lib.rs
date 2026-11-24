//! FrostRig desktop app. Runs as the normal user and never touches
//! hardware: every change is a D-Bus call to the frostrigd service.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};
use tokio::sync::OnceCell;
use zbus::Connection;

#[zbus::proxy(
    interface = "io.github.frostrig.Daemon1",
    default_service = "io.github.frostrig.Daemon",
    default_path = "/io/github/frostrig/Daemon"
)]
trait Daemon {
    fn get_status(&self) -> zbus::Result<String>;
    fn set_mode(&self, mode: &str) -> zbus::Result<()>;
    fn set_max_fans(&self, on: bool) -> zbus::Result<()>;
    fn set_charge_limit(&self, percent: u8) -> zbus::Result<()>;
    fn set_turbo(&self, on: bool) -> zbus::Result<()>;
    fn set_cpu_power(&self, sustained_w: u32, burst_w: u32) -> zbus::Result<()>;
    fn set_fan_preset(&self, preset: &str) -> zbus::Result<()>;
    fn set_custom_fan_curve(
        &self,
        cpu_temps: &[u8],
        cpu_speeds: &[u8],
        gpu_temps: &[u8],
        gpu_speeds: &[u8],
    ) -> zbus::Result<()>;
    fn reset_advanced(&self) -> zbus::Result<()>;
    fn get_report(&self) -> zbus::Result<String>;
    fn set_webcam(&self, on: bool) -> zbus::Result<()>;
    fn set_webcam_block(&self, on: bool) -> zbus::Result<()>;
    fn set_fn_key(&self, side: &str) -> zbus::Result<()>;
    fn set_super_battery(&self, on: bool) -> zbus::Result<()>;
    fn set_keyboard_backlight(&self, level: u32) -> zbus::Result<()>;
    fn set_gpu_clock_limit(&self, gpu: &str, mhz: u32) -> zbus::Result<()>;
    fn set_gpu_power_limit(&self, gpu: &str, watts: u32) -> zbus::Result<()>;
    fn set_gpu_level(&self, gpu: &str, level: &str) -> zbus::Result<()>;
    fn set_dynamic_boost(&self, on: bool) -> zbus::Result<()>;
    fn start_fan_test(&self) -> zbus::Result<()>;
    fn cancel_fan_test(&self) -> zbus::Result<()>;
}

static BUS: OnceCell<Connection> = OnceCell::const_new();
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(true);
static TRAY_READY: AtomicBool = AtomicBool::new(false);

/// Turn D-Bus errors into something a person can act on.
fn friendly(e: zbus::Error) -> String {
    let text = e.to_string();
    if text.contains("ServiceUnknown") || text.contains("NameHasNoOwner") || text.contains("was not provided") {
        return "The FrostRig service isn't running. Start it with: sudo systemctl enable --now frostrigd".into();
    }
    match e {
        zbus::Error::MethodError(_, Some(msg), _) => msg,
        zbus::Error::FDO(fdo) => match *fdo {
            zbus::fdo::Error::Failed(msg)
            | zbus::fdo::Error::AccessDenied(msg)
            | zbus::fdo::Error::InvalidArgs(msg) => msg,
            other => other.to_string(),
        },
        _ => text,
    }
}

async fn daemon() -> Result<DaemonProxy<'static>, String> {
    let conn = BUS.get_or_try_init(|| async { Connection::system().await }).await.map_err(friendly)?;
    DaemonProxy::new(conn).await.map_err(friendly)
}

#[tauri::command]
async fn status() -> Result<String, String> {
    daemon().await?.get_status().await.map_err(friendly)
}

#[tauri::command]
async fn set_mode(mode: String) -> Result<(), String> {
    daemon().await?.set_mode(&mode).await.map_err(friendly)
}

#[tauri::command]
async fn set_max_fans(on: bool) -> Result<(), String> {
    daemon().await?.set_max_fans(on).await.map_err(friendly)
}

#[tauri::command]
async fn set_charge_limit(percent: u8) -> Result<(), String> {
    daemon().await?.set_charge_limit(percent).await.map_err(friendly)
}

#[tauri::command]
async fn set_turbo(on: bool) -> Result<(), String> {
    daemon().await?.set_turbo(on).await.map_err(friendly)
}

#[tauri::command]
async fn set_cpu_power(sustained_w: u32, burst_w: u32) -> Result<(), String> {
    daemon().await?.set_cpu_power(sustained_w, burst_w).await.map_err(friendly)
}

#[tauri::command]
async fn set_fan_preset(preset: String) -> Result<(), String> {
    daemon().await?.set_fan_preset(&preset).await.map_err(friendly)
}

#[tauri::command]
async fn set_custom_fan_curve(
    cpu_temps: Vec<u8>,
    cpu_speeds: Vec<u8>,
    gpu_temps: Vec<u8>,
    gpu_speeds: Vec<u8>,
) -> Result<(), String> {
    daemon().await?
        .set_custom_fan_curve(&cpu_temps, &cpu_speeds, &gpu_temps, &gpu_speeds)
        .await
        .map_err(friendly)
}

#[tauri::command]
async fn reset_advanced() -> Result<(), String> {
    daemon().await?.reset_advanced().await.map_err(friendly)
}

#[tauri::command]
async fn set_webcam(on: bool) -> Result<(), String> {
    daemon().await?.set_webcam(on).await.map_err(friendly)
}

#[tauri::command]
async fn set_webcam_block(on: bool) -> Result<(), String> {
    daemon().await?.set_webcam_block(on).await.map_err(friendly)
}

#[tauri::command]
async fn set_fn_key(side: String) -> Result<(), String> {
    daemon().await?.set_fn_key(&side).await.map_err(friendly)
}

#[tauri::command]
async fn set_super_battery(on: bool) -> Result<(), String> {
    daemon().await?.set_super_battery(on).await.map_err(friendly)
}

#[tauri::command]
async fn set_keyboard_backlight(level: u32) -> Result<(), String> {
    daemon().await?.set_keyboard_backlight(level).await.map_err(friendly)
}

#[tauri::command]
async fn report() -> Result<String, String> {
    daemon().await?.get_report().await.map_err(friendly)
}

#[tauri::command]
async fn set_gpu_clock_limit(gpu: String, mhz: u32) -> Result<(), String> {
    daemon().await?.set_gpu_clock_limit(&gpu, mhz).await.map_err(friendly)
}

#[tauri::command]
async fn set_gpu_power_limit(gpu: String, watts: u32) -> Result<(), String> {
    daemon().await?.set_gpu_power_limit(&gpu, watts).await.map_err(friendly)
}

#[tauri::command]
async fn set_gpu_level(gpu: String, level: String) -> Result<(), String> {
    daemon().await?.set_gpu_level(&gpu, &level).await.map_err(friendly)
}

#[tauri::command]
async fn set_dynamic_boost(on: bool) -> Result<(), String> {
    daemon().await?.set_dynamic_boost(on).await.map_err(friendly)
}

#[tauri::command]
async fn start_fan_test() -> Result<(), String> {
    daemon().await?.start_fan_test().await.map_err(friendly)
}

#[tauri::command]
async fn cancel_fan_test() -> Result<(), String> {
    daemon().await?.cancel_fan_test().await.map_err(friendly)
}

// ---------- app-only settings (no hardware) ----------

#[tauri::command]
fn set_close_to_tray(on: bool) {
    CLOSE_TO_TRAY.store(on, Ordering::Relaxed);
}

fn autostart_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join("frostrig.desktop"))
}

#[tauri::command]
fn get_autostart() -> bool {
    autostart_file().is_some_and(|p| p.exists())
}

#[tauri::command]
fn set_autostart(on: bool) -> Result<(), String> {
    let path = autostart_file().ok_or("Can't find your config folder.")?;
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=FrostRig\nExec=\"{}\" --minimized\nIcon=frostrig\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, entry).map_err(|e| e.to_string())
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open FrostRig", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("FrostRig")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    TRAY_READY.store(true, Ordering::Relaxed);
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if let Err(e) = build_tray(app.handle()) {
                eprintln!("System tray unavailable: {e}");
            }
            // Started from autostart: stay in the tray.
            if std::env::args().any(|a| a == "--minimized") && TRAY_READY.load(Ordering::Relaxed) {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Only hide if there's a tray icon to bring it back from.
                if CLOSE_TO_TRAY.load(Ordering::Relaxed) && TRAY_READY.load(Ordering::Relaxed) {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            status,
            set_mode,
            set_max_fans,
            set_charge_limit,
            set_turbo,
            set_cpu_power,
            set_fan_preset,
            set_custom_fan_curve,
            reset_advanced,
            start_fan_test,
            cancel_fan_test,
            report,
            set_webcam,
            set_webcam_block,
            set_fn_key,
            set_super_battery,
            set_keyboard_backlight,
            set_gpu_clock_limit,
            set_gpu_power_limit,
            set_gpu_level,
            set_dynamic_boost,
            set_close_to_tray,
            get_autostart,
            set_autostart
        ])
        .run(tauri::generate_context!())
        .expect("FrostRig failed to start");
}
