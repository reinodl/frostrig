//! frostrigd: the one process that changes hardware.
//!
//! Started by systemd at boot. Restores the user's settings, then serves
//! `io.github.frostrig.Daemon1` on the system bus. Reading status is open to
//! everyone; changing anything is checked with polkit, which lets the person
//! sitting at the laptop through without a password prompt.

use futures_util::StreamExt;
use frostrig_core::caps::MSI_EC;
use frostrig_core::fan::{Curve, Preset, Tables, POINTS};
use frostrig_core::state::STATE_DIR;
use frostrig_core::{Controller, Hw, Mode};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::message::Header;
use zbus::zvariant::Value;
use zbus::{fdo, interface, Connection};

const BUS_NAME: &str = "io.github.frostrig.Daemon";
const OBJECT_PATH: &str = "/io/github/frostrig/Daemon";
const POLKIT_ACTION: &str = "io.github.frostrig.manage";

#[zbus::proxy(
    interface = "org.freedesktop.PolicyKit1.Authority",
    default_service = "org.freedesktop.PolicyKit1",
    default_path = "/org/freedesktop/PolicyKit1/Authority"
)]
trait Authority {
    fn check_authorization(
        &self,
        subject: &(&str, HashMap<&str, Value<'_>>),
        action_id: &str,
        details: &HashMap<&str, &str>,
        flags: u32,
        cancellation_id: &str,
    ) -> zbus::Result<(bool, bool, HashMap<String, String>)>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

fn log(msg: impl AsRef<str>) {
    // stderr goes to the journal: `journalctl -u frostrigd`
    eprintln!("{}", msg.as_ref());
}

async fn authorize(conn: &Connection, header: &Header<'_>) -> fdo::Result<()> {
    let sender = header.sender().ok_or_else(|| fdo::Error::AccessDenied("Unknown caller.".into()))?;
    let authority = AuthorityProxy::new(conn).await.map_err(|e| fdo::Error::Failed(format!("polkit: {e}")))?;
    let subject: HashMap<&str, Value<'_>> = HashMap::from([("name", Value::from(sender.as_str()))]);
    let details: HashMap<&str, &str> = HashMap::new();
    // Flag 1 = allow interaction: someone not at the laptop (e.g. over SSH)
    // gets a password prompt; the person at the laptop does not.
    let (authorized, _, _) = authority
        .check_authorization(&("system-bus-name", subject), POLKIT_ACTION, &details, 1, "")
        .await
        .map_err(|e| fdo::Error::Failed(format!("polkit: {e}")))?;
    if authorized {
        Ok(())
    } else {
        Err(fdo::Error::AccessDenied("Only the person logged in at this laptop can change its settings.".into()))
    }
}

struct Daemon {
    ctl: Arc<Mutex<Controller>>,
}

impl Daemon {
    fn with(&self, f: impl FnOnce(&mut Controller) -> frostrig_core::Result<()>) -> fdo::Result<()> {
        let mut ctl = self.ctl.lock().map_err(|_| fdo::Error::Failed("Service state is unavailable.".into()))?;
        f(&mut ctl).map_err(|e| {
            log(format!("refused: {e}"));
            fdo::Error::Failed(e.0)
        })
    }
}

fn curve(temps: Vec<u8>, speeds: Vec<u8>) -> fdo::Result<Curve> {
    let bad = || fdo::Error::InvalidArgs(format!("A fan curve needs exactly {POINTS} points."));
    Ok(Curve { temps: temps.try_into().map_err(|_| bad())?, speeds: speeds.try_into().map_err(|_| bad())? })
}

#[interface(name = "io.github.frostrig.Daemon1")]
impl Daemon {
    /// Everything the app shows, as one JSON document.
    async fn get_status(&self) -> fdo::Result<String> {
        let ctl = self.ctl.lock().map_err(|_| fdo::Error::Failed("Service state is unavailable.".into()))?;
        Ok(ctl.status_json())
    }

    async fn set_mode(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        mode: String,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        let mode = Mode::parse(&mode).ok_or_else(|| fdo::Error::InvalidArgs(format!("Unknown mode {mode:?}.")))?;
        self.with(|c| c.set_mode(mode))
    }

    async fn set_max_fans(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        on: bool,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_cooler_boost(on))
    }

    async fn set_webcam(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        on: bool,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_webcam(on))
    }

    async fn set_webcam_block(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        on: bool,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_webcam_block(on))
    }

    /// "left" or "right".
    async fn set_fn_key(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        side: String,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_fn_key(&side))
    }

    async fn set_super_battery(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        on: bool,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_super_battery(on))
    }

    async fn set_keyboard_backlight(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        level: u32,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_kbd_backlight(level as u64))
    }

    async fn set_charge_limit(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        percent: u8,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_charge_limit(percent))
    }

    async fn set_turbo(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        on: bool,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_turbo(on))
    }

    async fn set_cpu_power(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        sustained_w: u32,
        burst_w: u32,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_cpu_power(sustained_w, burst_w))
    }

    async fn set_fan_preset(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        preset: String,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        let p = Preset::parse(&preset).ok_or_else(|| fdo::Error::InvalidArgs(format!("Unknown fan curve {preset:?}.")))?;
        self.with(|c| c.set_fan_preset(p))
    }

    async fn set_custom_fan_curve(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        cpu_temps: Vec<u8>,
        cpu_speeds: Vec<u8>,
        gpu_temps: Vec<u8>,
        gpu_speeds: Vec<u8>,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        let t = Tables { cpu: curve(cpu_temps, cpu_speeds)?, gpu: curve(gpu_temps, gpu_speeds)? };
        self.with(|c| c.set_custom_curve(t))
    }

    /// Plain-text detection summary for bug reports. Open to everyone, like status.
    async fn get_report(&self) -> fdo::Result<String> {
        let ctl = self.ctl.lock().map_err(|_| fdo::Error::Failed("Service state is unavailable.".into()))?;
        Ok(ctl.report())
    }

    /// GPU clock cap in MHz; 0 removes the cap. `gpu` is the PCI address from status.
    async fn set_gpu_clock_limit(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        gpu: String,
        mhz: u32,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_gpu_clock_limit(&gpu, if mhz == 0 { None } else { Some(mhz) }))
    }

    /// GPU power limit in watts; 0 goes back to the driver's default.
    async fn set_gpu_power_limit(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        gpu: String,
        watts: u32,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_gpu_power_limit(&gpu, if watts == 0 { None } else { Some(watts) }))
    }

    /// AMD GPUs: "auto", "low" or "high".
    async fn set_gpu_level(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        gpu: String,
        level: String,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_gpu_level(&gpu, &level))
    }

    async fn set_dynamic_boost(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        on: bool,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.set_dynamic_boost(on))
    }

    async fn reset_advanced(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.reset_advanced())
    }

    /// Fan test that unlocks fan curves on MSI models other than the built-in
    /// verified one. Briefly raises fan speed, then puts everything back.
    async fn start_fan_test(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| c.start_fan_test())?;
        log("Fan test started.");
        Ok(())
    }

    async fn cancel_fan_test(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<()> {
        authorize(conn, &header).await?;
        self.with(|c| { c.cancel_fan_test(); Ok(()) })
    }

    #[zbus(property)]
    async fn version(&self) -> String {
        frostrig_core::VERSION.to_owned()
    }
}

/// Wait for the msi-ec driver at boot; give up quietly on laptops without it.
async fn wait_for_msi_ec(hw: &Hw) {
    for _ in 0..30 {
        if hw.exists(&format!("{MSI_EC}/fw_version")) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    log("msi-ec driver not found; MSI-specific features will be unavailable.");
}

/// Before the rename the service kept its files in /var/lib/msi-control.
/// Bring them across once so the recorded factory values and the user's
/// settings survive the switch.
fn import_legacy_state(hw: &Hw) {
    for name in ["settings", "factory"] {
        let new = format!("{STATE_DIR}/{name}");
        let old = format!("/var/lib/msi-control/{name}");
        if hw.exists(&new) {
            continue;
        }
        if let Some(text) = hw.read(&old) {
            match hw.save_private(&new, &format!("{text}\n")) {
                Ok(()) => log(format!("Imported {name} from the MSI Control install.")),
                Err(e) => log(format!("Could not import {old}: {e}")),
            }
        }
    }
}

fn restore(ctl: &Mutex<Controller>, why: &str) {
    let Ok(ctl) = ctl.lock() else { return };
    let errors = ctl.restore();
    if errors.is_empty() {
        log(format!("Settings restored ({why})."));
    }
    for e in errors {
        log(format!("Could not restore after {why}: {e}"));
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{}", frostrig_core::VERSION);
        return Ok(());
    }

    let hw = Hw::system();
    wait_for_msi_ec(&hw).await;
    import_legacy_state(&hw);
    let ctl = Arc::new(Mutex::new(Controller::open(hw)));
    restore(&ctl, "boot");

    let conn = zbus::connection::Builder::system()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, Daemon { ctl: ctl.clone() })?
        .build()
        .await?;
    log(format!("frostrigd {} ready.", frostrig_core::VERSION));

    // The NVIDIA driver forgets the clock limit whenever the GPU powers off.
    // Checking only reads sysfs, so this never wakes a sleeping GPU.
    let watcher = ctl.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            let messages = match watcher.lock() {
                Ok(mut c) => {
                    let mut m = c.fan_test_tick();
                    m.extend(c.gpu_watch());
                    m
                }
                Err(_) => Vec::new(),
            };
            for m in messages {
                log(m);
            }
        }
    });

    // The EC forgets some settings over sleep; put them back on wake.
    let login = Login1ManagerProxy::new(&conn).await?;
    let mut sleeps = login.receive_prepare_for_sleep().await?;
    while let Some(signal) = sleeps.next().await {
        if let Ok(args) = signal.args() {
            if !*args.start() {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if let Ok(mut c) = ctl.lock() {
                    c.redetect();
                }
                restore(&ctl, "resume");
            }
        }
    }
    Ok(())
}
