//! Tests against a fake sysfs tree. No real hardware is touched.

use crate::caps::{self, EC_IO};
use crate::fan::{self, Curve, Preset, Tables};
use crate::state::{Settings, FACTORY_FILE, SETTINGS_FILE};
use crate::{Controller, Hw, Mode};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

/// Synthetic tables in the GT77 layout. Not anyone's real curve.
const CPU: Curve = Curve { temps: [40, 48, 56, 64, 72, 82, 100], speeds: [30, 36, 44, 57, 71, 86, 100] };
const GPU: Curve = Curve { temps: [42, 50, 58, 66, 74, 84, 100], speeds: [30, 38, 48, 60, 75, 88, 100] };

struct Fake {
    root: PathBuf,
}

impl Fake {
    fn new() -> Fake {
        let root = std::env::temp_dir().join(format!("msi-core-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Fake { root }
    }

    fn put(&self, abs: &str, v: &str) -> &Self {
        let p = self.root.join(abs.trim_start_matches('/'));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, v).unwrap();
        self
    }

    fn get(&self, abs: &str) -> String {
        fs::read_to_string(self.root.join(abs.trim_start_matches('/'))).unwrap().trim().to_owned()
    }

    fn hw(&self) -> Hw {
        Hw::at(&self.root)
    }

    fn ec_bytes(&self, t: &Tables) {
        let mut io = vec![0u8; 256];
        io[106..113].copy_from_slice(&t.cpu.temps);
        io[114..121].copy_from_slice(&t.cpu.speeds);
        io[130..137].copy_from_slice(&t.gpu.temps);
        io[138..145].copy_from_slice(&t.gpu.speeds);
        let p = self.root.join(EC_IO.trim_start_matches('/'));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, io).unwrap();
    }

    /// A Titan GT77HX 13VI with msi-ec, ec_sys (writable), intel_pstate and RAPL.
    fn gt77() -> Fake {
        let f = Fake::new();
        f.put("/sys/class/dmi/id/product_name", "Titan GT77HX 13VI\n")
            .put("/sys/class/dmi/id/board_name", "MS-17Q2\n")
            .put("/sys/devices/platform/msi-ec/fw_version", "17Q2IMS1.10E\n")
            .put("/sys/devices/platform/msi-ec/shift_mode", "comfort\n")
            .put("/sys/devices/platform/msi-ec/available_shift_modes", "eco\ncomfort\nsport\nturbo\n")
            .put("/sys/devices/platform/msi-ec/fan_mode", "auto\n")
            .put("/sys/devices/platform/msi-ec/available_fan_modes", "auto\nsilent\nadvanced\n")
            .put("/sys/devices/platform/msi-ec/cooler_boost", "off\n")
            .put("/sys/devices/platform/msi-ec/webcam", "on\n")
            .put("/sys/devices/platform/msi-ec/webcam_block", "off\n")
            .put("/sys/devices/platform/msi-ec/fn_key", "left\n")
            .put("/sys/devices/platform/msi-ec/win_key", "right\n")
            .put("/sys/devices/platform/msi-ec/super_battery", "off\n")
            .put("/sys/devices/platform/msi-ec/cpu/realtime_temperature", "52\n")
            .put("/sys/devices/platform/msi-ec/gpu/realtime_temperature", "47\n")
            .put("/sys/devices/platform/msi-ec/cpu/realtime_fan_speed", "40\n")
            .put("/sys/devices/platform/msi-ec/gpu/realtime_fan_speed", "38\n")
            .put("/sys/class/power_supply/BAT1/type", "Battery\n")
            .put("/sys/class/power_supply/BAT1/capacity", "84\n")
            .put("/sys/class/power_supply/BAT1/status", "Charging\n")
            .put("/sys/class/power_supply/BAT1/charge_control_start_threshold", "90\n")
            .put("/sys/class/power_supply/BAT1/charge_control_end_threshold", "100\n")
            .put("/sys/class/power_supply/ADP1/type", "Mains\n")
            .put("/sys/class/power_supply/ADP1/online", "1\n")
            .put("/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver", "intel_pstate\n")
            .put("/sys/devices/system/cpu/intel_pstate/no_turbo", "0\n")
            .put("/sys/class/powercap/intel-rapl:0/name", "package-0\n")
            .put("/sys/class/powercap/intel-rapl:0/constraint_0_name", "long_term\n")
            .put("/sys/class/powercap/intel-rapl:0/constraint_1_name", "short_term\n")
            .put("/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw", "95000000\n")
            .put("/sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw", "135000000\n")
            .put("/sys/class/powercap/intel-rapl:0:0/name", "core\n")
            .put("/sys/module/ec_sys/parameters/write_support", "Y\n")
            .put("/sys/class/hwmon/hwmon5/name", "msi_wmi_platform\n")
            .put("/sys/class/hwmon/hwmon5/fan1_input", "2900\n")
            .put("/sys/class/hwmon/hwmon5/fan2_input", "2700\n");
        f.ec_bytes(&Tables { cpu: CPU, gpu: GPU });
        f
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const EC: &str = "/sys/devices/platform/msi-ec";

#[test]
fn detects_everything_on_the_gt77() {
    let f = Fake::gt77();
    let c = caps::detect(&f.hw());
    assert!(c.is_gt77());
    assert_eq!(c.modes.iter().map(|m| m.0).collect::<Vec<_>>(), Mode::ALL.to_vec());
    assert!(c.cooler_boost);
    assert_eq!(c.battery.as_deref(), Some("/sys/class/power_supply/BAT1"));
    assert!(!c.battery_linked_start, "msi-ec: only the end threshold is written");
    assert_eq!(c.turbo, Some(caps::TurboSwitch::IntelNoTurbo));
    assert_eq!(c.rapl.as_deref(), Some("/sys/class/powercap/intel-rapl:0"));
    let ctl = Controller::open(f.hw());
    assert!(ctl.caps().fan_curves, "{:?}", ctl.caps().fan_curve_reason);
    assert!(ctl.caps().fan_builtin);
}

#[test]
fn other_msi_laptops_get_modes_but_never_fan_curves() {
    let f = Fake::gt77();
    f.put("/sys/class/dmi/id/product_name", "Katana GF66 11UE\n")
        .put(&format!("{EC}/available_shift_modes"), "eco\ncomfort\nsport\n");
    let c = caps::detect(&f.hw());
    assert_eq!(c.modes.iter().map(|m| m.0).collect::<Vec<_>>(), vec![Mode::Silent, Mode::Balanced, Mode::Performance]);
    let mut ctl = Controller::open(f.hw());
    assert!(!ctl.caps().fan_curves);
    assert!(ctl.caps().fan_curve_reason.clone().unwrap().contains("verified"));
    assert!(ctl.set_mode(Mode::Extreme).is_err());
    assert!(ctl.set_fan_preset(Preset::Cool).is_err());
}

#[test]
fn fan_curves_need_writable_ec_sys_and_exact_firmware() {
    let f = Fake::gt77();
    f.put("/sys/module/ec_sys/parameters/write_support", "N\n");
    assert!(!Controller::open(f.hw()).caps().fan_curves);
    let f = Fake::gt77();
    f.put(&format!("{EC}/fw_version"), "17Q2IMS1.10F\n");
    assert!(!Controller::open(f.hw()).caps().fan_curves);
}

#[test]
fn no_msi_ec_means_no_modes_and_no_crash() {
    let f = Fake::new();
    f.put("/sys/class/power_supply/BAT0/type", "Battery\n")
        .put("/sys/class/power_supply/BAT0/charge_control_start_threshold", "0\n")
        .put("/sys/class/power_supply/BAT0/charge_control_end_threshold", "100\n");
    let ctl = Controller::open(f.hw());
    assert!(ctl.caps().modes.is_empty());
    assert!(ctl.caps().battery_linked_start);
    let s = ctl.status_json();
    assert!(s.contains(r#""modes":[]"#), "{s}");
}

#[test]
fn modes_set_shift_and_fan_mode_and_persist() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_mode(Mode::Silent).unwrap();
    assert_eq!(f.get(&format!("{EC}/shift_mode")), "eco");
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "silent");
    ctl.set_mode(Mode::Extreme).unwrap();
    assert_eq!(f.get(&format!("{EC}/shift_mode")), "turbo");
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "auto");
    assert!(f.get(SETTINGS_FILE).contains("mode=extreme"));
    assert_eq!(Controller::open(f.hw()).settings().mode, Some(Mode::Extreme));
}

#[test]
fn charge_limit_only_accepts_the_three_choices() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    assert!(ctl.set_charge_limit(70).is_err());
    ctl.set_charge_limit(80).unwrap();
    assert_eq!(f.get("/sys/class/power_supply/BAT1/charge_control_end_threshold"), "80");
    assert_eq!(f.get("/sys/class/power_supply/BAT1/charge_control_start_threshold"), "90", "msi-ec start is never written");
}

#[test]
fn separate_start_threshold_stays_ten_below_end() {
    let f = Fake::new();
    f.put("/sys/class/power_supply/BAT0/type", "Battery\n")
        .put("/sys/class/power_supply/BAT0/charge_control_start_threshold", "90\n")
        .put("/sys/class/power_supply/BAT0/charge_control_end_threshold", "100\n");
    let mut ctl = Controller::open(f.hw());
    ctl.set_charge_limit(60).unwrap();
    assert_eq!(f.get("/sys/class/power_supply/BAT0/charge_control_start_threshold"), "50");
    assert_eq!(f.get("/sys/class/power_supply/BAT0/charge_control_end_threshold"), "60");
    ctl.set_charge_limit(100).unwrap();
    assert_eq!(f.get("/sys/class/power_supply/BAT0/charge_control_start_threshold"), "90");
}

#[test]
fn turbo_uses_inverted_intel_switch() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_turbo(false).unwrap();
    assert_eq!(f.get("/sys/devices/system/cpu/intel_pstate/no_turbo"), "1");
    ctl.set_turbo(true).unwrap();
    assert_eq!(f.get("/sys/devices/system/cpu/intel_pstate/no_turbo"), "0");
}

#[test]
fn cpu_power_never_goes_above_factory_and_resets() {
    let f = Fake::gt77();
    let rapl = "/sys/class/powercap/intel-rapl:0";
    let mut ctl = Controller::open(f.hw());
    let l = ctl.power_limits().unwrap();
    assert_eq!((l.pl1_max, l.pl2_max), (95, 135));
    assert!(ctl.set_cpu_power(100, 135).is_err(), "above factory PL1");
    assert!(ctl.set_cpu_power(10, 135).is_err(), "below minimum");
    assert!(ctl.set_cpu_power(90, 80).is_err(), "PL1 above PL2");
    ctl.set_cpu_power(45, 60).unwrap();
    assert_eq!(f.get(&format!("{rapl}/constraint_0_power_limit_uw")), "45000000");
    assert_eq!(f.get(&format!("{rapl}/constraint_1_power_limit_uw")), "60000000");
    // Factory values were recorded before the change and survive a restart.
    let mut ctl = Controller::open(f.hw());
    assert_eq!(ctl.power_limits().unwrap().pl1_max, 95);
    ctl.reset_advanced().unwrap();
    assert_eq!(f.get(&format!("{rapl}/constraint_0_power_limit_uw")), "95000000");
    assert_eq!(f.get(&format!("{rapl}/constraint_1_power_limit_uw")), "135000000");
    assert_eq!(ctl.settings().cpu_power, None);
}

#[test]
fn factory_is_recorded_once_before_any_change() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_turbo(false).unwrap();
    let saved = f.get(FACTORY_FILE);
    assert!(saved.contains("turbo=1"), "{saved}");
    assert!(saved.contains("factory_cpu_speeds=30,36,44,57,71,86,100"), "{saved}");
    drop(ctl);
    Controller::open(f.hw());
    assert_eq!(f.get(FACTORY_FILE), saved);
}

#[test]
fn presets_derive_from_factory_tables() {
    let factory = Tables { cpu: CPU, gpu: GPU };
    let quiet = fan::preset(&factory, Preset::Quiet).unwrap();
    assert_eq!(quiet.cpu.speeds, [30, 30, 34, 47, 71, 86, 100], "only below 70°C, floor 30%");
    assert_eq!(quiet.cpu.temps, CPU.temps);
    let cool = fan::preset(&factory, Preset::Cool).unwrap();
    assert_eq!(cool.cpu.speeds, [40, 46, 54, 67, 81, 96, 100]);
    assert_eq!(fan::preset(&factory, Preset::Auto).unwrap(), factory);
    assert!(fan::preset(&factory, Preset::Custom).is_none());
}

#[test]
fn fan_preset_writes_ec_and_auto_restores_factory() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_fan_preset(Preset::Cool).unwrap();
    let live = fan::read(&f.hw(), fan::FULL).unwrap();
    assert_eq!(live.gpu.speeds, [40, 48, 58, 70, 85, 98, 100]);
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "advanced");
    ctl.set_fan_preset(Preset::Auto).unwrap();
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), Tables { cpu: CPU, gpu: GPU });
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "auto");
}

#[test]
fn quieter_curves_are_refused_when_hot_but_cooler_ones_are_not() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    f.put(&format!("{EC}/cpu/realtime_temperature"), "88\n");
    let err = ctl.set_fan_preset(Preset::Quiet).unwrap_err();
    assert!(err.0.contains("88"), "{err}");
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), Tables { cpu: CPU, gpu: GPU }, "nothing written");
    ctl.set_fan_preset(Preset::Cool).unwrap();
}

#[test]
fn custom_curves_are_validated() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    let good = Tables {
        cpu: Curve { temps: [40, 50, 60, 70, 80, 90, 100], speeds: [30, 35, 45, 60, 75, 90, 100] },
        gpu: Curve { temps: [40, 50, 60, 70, 80, 90, 100], speeds: [30, 35, 45, 60, 75, 90, 100] },
    };
    let mut bad = good;
    bad.cpu.speeds[3] = 20;
    assert!(ctl.set_custom_curve(bad).is_err());
    let mut bad = good;
    bad.gpu.speeds[4] = 50; // drops as temperature rises
    assert!(ctl.set_custom_curve(bad).is_err());
    let mut bad = good;
    bad.cpu.temps[6] = 95;
    assert!(ctl.set_custom_curve(bad).is_err());
    ctl.set_custom_curve(good).unwrap();
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), good);
    let s = Controller::open(f.hw()).settings().clone();
    assert_eq!(s.fan_preset, Some(Preset::Custom));
    assert_eq!(s.custom_curve, Some(good));
}

#[test]
fn unexpected_ec_layout_is_never_written() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    let mut odd = Tables { cpu: CPU, gpu: GPU };
    odd.cpu.temps = [0, 0, 0, 0, 0, 0, 0];
    f.ec_bytes(&odd);
    assert!(ctl.set_fan_preset(Preset::Cool).is_err());
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), odd);
}

#[test]
fn restore_puts_everything_back_after_a_reboot() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_mode(Mode::Performance).unwrap();
    ctl.set_cooler_boost(true).unwrap();
    ctl.set_charge_limit(60).unwrap();
    ctl.set_turbo(false).unwrap();
    ctl.set_cpu_power(80, 120).unwrap();
    ctl.set_fan_preset(Preset::Cool).unwrap();
    let wanted = fan::read(&f.hw(), fan::FULL).unwrap();
    drop(ctl);

    // "Reboot": firmware puts its defaults back.
    f.put(&format!("{EC}/shift_mode"), "comfort\n")
        .put(&format!("{EC}/fan_mode"), "auto\n")
        .put(&format!("{EC}/cooler_boost"), "off\n")
        .put("/sys/class/power_supply/BAT1/charge_control_end_threshold", "100\n")
        .put("/sys/devices/system/cpu/intel_pstate/no_turbo", "0\n")
        .put("/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw", "95000000\n")
        .put("/sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw", "135000000\n");
    f.ec_bytes(&Tables { cpu: CPU, gpu: GPU });

    let ctl = Controller::open(f.hw());
    assert_eq!(ctl.restore(), Vec::<String>::new());
    assert_eq!(f.get(&format!("{EC}/shift_mode")), "sport");
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "advanced");
    assert_eq!(f.get(&format!("{EC}/cooler_boost")), "on");
    assert_eq!(f.get("/sys/class/power_supply/BAT1/charge_control_end_threshold"), "60");
    assert_eq!(f.get("/sys/devices/system/cpu/intel_pstate/no_turbo"), "1");
    assert_eq!(f.get("/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw"), "80000000");
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), wanted);
}

#[test]
fn restore_reports_failures_without_stopping() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_mode(Mode::Performance).unwrap();
    ctl.set_charge_limit(80).unwrap();
    drop(ctl);
    // After an update, the driver no longer offers "sport".
    f.put(&format!("{EC}/available_shift_modes"), "eco\ncomfort\n")
        .put("/sys/class/power_supply/BAT1/charge_control_end_threshold", "100\n");
    let errors = Controller::open(f.hw()).restore();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("Mode"));
    assert_eq!(f.get("/sys/class/power_supply/BAT1/charge_control_end_threshold"), "80");
}

#[test]
fn settings_round_trip() {
    let s = Settings {
        mode: Some(Mode::Silent),
        cooler_boost: Some(false),
        charge_limit: Some(80),
        turbo: Some(true),
        cpu_power: Some((60, 90)),
        fan_preset: Some(Preset::Custom),
        custom_curve: Some(Tables { cpu: CPU, gpu: GPU }),
        webcam: Some(false),
        webcam_block: Some(true),
        fn_key: Some("right".into()),
        super_battery: Some(true),
        kbd_backlight: Some(2),
        gpus: [("0000:01:00.0".to_owned(), crate::state::GpuSettings { clock_mhz: Some(1800), power_w: Some(200), level: Some("high".into()) })].into(),
        legacy_gpu_clock: None,
        dynamic_boost: Some(true),
    };
    assert_eq!(Settings::parse(&s.serialize()), s);
    assert_eq!(Settings::parse("charge_limit=70\nmode=ludicrous\n"), Settings::default());
}

#[test]
fn status_reports_live_values() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_mode(Mode::Performance).unwrap();
    let s = ctl.status_json();
    for needle in [
        r#""model":"Titan GT77HX 13VI""#,
        r#""modes":["silent","balanced","performance","extreme"]"#,
        r#""fanCurves":true"#,
        r#""mode":"performance""#,
        r#""turbo":true"#,
        r#""pl1W":95"#,
        r#""pl1Max":95"#,
        r#""cpuTemp":52"#,
        r#""fans":[{"label":"Fan 1","rpm":2900},{"label":"Fan 2","rpm":2700}]"#,
        r#""percent":84"#,
        r#""acOnline":true"#,
        r#""chargeLimit":100"#,
        r#""speeds":[30,36,44,57,71,86,100]"#,
    ] {
        assert!(s.contains(needle), "missing {needle} in {s}");
    }
}

#[test]
fn fans_are_found_on_any_hwmon_device_in_order() {
    let f = Fake::new();
    f.put("/sys/class/hwmon/hwmon10/name", "amdgpu\n")
        .put("/sys/class/hwmon/hwmon10/fan1_input", "1200\n")
        .put("/sys/class/hwmon/hwmon2/name", "thinkpad\n")
        .put("/sys/class/hwmon/hwmon2/fan1_input", "3100\n")
        .put("/sys/class/hwmon/hwmon2/fan2_input", "0\n")
        .put("/sys/class/hwmon/hwmon2/fan2_label", "Exhaust\n")
        .put("/sys/class/hwmon/hwmon2/fan10_input", "900\n")
        .put("/sys/class/hwmon/hwmon3/name", "coretemp\n")
        .put("/sys/class/hwmon/hwmon3/temp1_input", "45000\n");
    let fans = Controller::open(f.hw()).fans();
    assert_eq!(fans, vec![
        ("Fan 1".to_owned(), 3100),
        ("Exhaust".to_owned(), 0),
        ("Fan 3".to_owned(), 900),
        ("GPU fan".to_owned(), 1200),
    ]);
}

#[test]
fn four_fan_laptop_reports_all_four() {
    let f = Fake::gt77();
    f.put("/sys/class/hwmon/hwmon5/fan3_input", "3300\n")
        .put("/sys/class/hwmon/hwmon5/fan4_input", "3150\n");
    assert_eq!(Controller::open(f.hw()).fans().len(), 4);
}

// ---------- GPU ----------

use crate::gpu::{Runner, NVIDIA_SMI, SYSTEMCTL};
use std::sync::{Arc, Mutex};

/// Stands in for nvidia-smi and systemctl, and records every call.
#[derive(Clone, Default)]
struct FakeRun {
    calls: Arc<Mutex<Vec<String>>>,
    boost: Arc<Mutex<bool>>,
    boost_enabled_at_boot: bool,
    /// power-profiles-daemon's current profile; None = not running.
    ppd: Arc<Mutex<Option<String>>>,
    /// What nvidia-smi says to a power-limit change.
    pl_reply: Arc<Mutex<Option<String>>>,
    /// nvidia-smi --query-gpu reply override.
    query_reply: Arc<Mutex<Option<String>>>,
}

impl FakeRun {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn clear(&self) {
        self.calls.lock().unwrap().clear();
    }
    fn smi_calls(&self) -> usize {
        self.calls().iter().filter(|c| c.starts_with(NVIDIA_SMI)).count()
    }
}

impl Runner for FakeRun {
    fn run(&self, program: &str, args: &[&str]) -> crate::Result<String> {
        self.calls.lock().unwrap().push(format!("{program} {}", args.join(" ")));
        if program == crate::caps::POWERPROFILESCTL {
            let mut ppd = self.ppd.lock().unwrap();
            let Some(cur) = ppd.clone() else { return Err(crate::Error::new("Failed to connect")) };
            return match args[0] {
                "list" => Ok(["performance", "balanced", "power-saver"].iter()
                    .map(|p| format!("{} {p}:\n    CpuDriver:\tintel_pstate\n    Degraded:   no\n\n", if *p == cur { "*" } else { " " }))
                    .collect()),
                "get" => Ok(format!("{cur}\n")),
                "set" => { *ppd = Some(args[1].to_owned()); Ok(String::new()) }
                _ => Err(crate::Error::new("unexpected")),
            };
        }
        if program == NVIDIA_SMI {
            if let Some(q) = args.iter().find(|a| a.starts_with("--query-gpu")) {
                if q.contains("power.min_limit") {
                    return Ok(self.query_reply.lock().unwrap().clone()
                        .unwrap_or_else(|| "[N/A], 150.00, 5.00, 175.00\n".into()));
                }
                return Ok("NVIDIA GeForce RTX 4090 Laptop GPU, 142.50, 175.00, 2310, 3105, 61\n".into());
            }
            if args.contains(&"-pl") {
                return Ok(self.pl_reply.lock().unwrap().clone().unwrap_or_else(||
                    "Changing power management limit is not supported for GPU: 00000000:01:00.0.\nTreating as warning and moving on.\nAll done.\n".into()));
            }
            return Ok("All done.\n".into());
        }
        if program == SYSTEMCTL {
            let mut boost = self.boost.lock().unwrap();
            return match args[0] {
                "is-active" if *boost => Ok("active\n".into()),
                "is-active" => Err(crate::Error::new("inactive")),
                "is-enabled" if self.boost_enabled_at_boot => Ok("enabled\n".into()),
                "is-enabled" => Err(crate::Error::new("disabled")),
                "start" => { *boost = true; Ok(String::new()) }
                "stop" => { *boost = false; Ok(String::new()) }
                _ => Err(crate::Error::new("unexpected")),
            };
        }
        Err(crate::Error::new("unexpected program"))
    }
}

const GPU_PCI: &str = "/sys/bus/pci/devices/0000:01:00.0";
const NV: &str = "0000:01:00.0";

impl Fake {
    /// A PCI display controller bound to `driver`.
    fn gpu_dev(&self, pci: &str, vendor: &str, driver: &str) -> String {
        let dev = format!("/sys/bus/pci/devices/{pci}");
        self.put(&format!("{dev}/vendor"), &format!("{vendor}\n"))
            .put(&format!("{dev}/class"), "0x030000\n")
            .put(&format!("{dev}/power/runtime_status"), "active\n");
        let drv = self.root.join(format!("sys/bus/pci/drivers/{driver}"));
        fs::create_dir_all(&drv).unwrap();
        std::os::unix::fs::symlink(&drv, self.root.join(dev.trim_start_matches('/')).join("driver")).unwrap();
        dev
    }

    /// The GT77: RTX 4090 Laptop GPU on the NVIDIA driver, plus Intel UHD on i915.
    fn gt77_with_gpu(awake: bool) -> Fake {
        let f = Fake::gt77();
        f.put("/usr/bin/nvidia-smi", "").put("/usr/lib/systemd/system/nvidia-powerd.service", "");
        f.gpu_dev(NV, "0x10de", "nvidia");
        f.intel_igpu();
        f.gpu_awake(awake);
        f
    }

    fn intel_igpu(&self) {
        let dev = self.gpu_dev("0000:00:02.0", "0x8086", "i915");
        self.put(&format!("{dev}/drm/card1/gt_RP0_freq_mhz"), "1300\n")
            .put(&format!("{dev}/drm/card1/gt_RPn_freq_mhz"), "300\n")
            .put(&format!("{dev}/drm/card1/gt_min_freq_mhz"), "300\n")
            .put(&format!("{dev}/drm/card1/gt_max_freq_mhz"), "1300\n")
            .put(&format!("{dev}/drm/card1/gt_act_freq_mhz"), "450\n");
    }

    fn gpu_awake(&self, awake: bool) {
        self.put(&format!("{GPU_PCI}/power/runtime_status"), if awake { "active\n" } else { "suspended\n" });
    }
}

fn gpu_ctl(f: &Fake, run: &FakeRun) -> Controller {
    let mut c = Controller::open_with(f.hw(), Box::new(run.clone()));
    c.gpu_watch(); // as the service does right after starting
    c
}

fn has(s: &str, needles: &[&str]) {
    for n in needles {
        assert!(s.contains(n), "missing {n} in {s}");
    }
}

#[test]
fn titan_shows_only_the_nvidia_gpu_with_its_real_controls() {
    let f = Fake::gt77_with_gpu(true);
    let run = FakeRun::default();
    let ctl = gpu_ctl(&f, &run);
    assert_eq!(ctl.gpus().len(), 2, "both GPUs found");
    let shown = ctl.shown_gpus();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].pci, NV, "the built-in Intel GPU is hidden on a laptop with a dedicated one");
    assert!(f.get(FACTORY_FILE).contains("gpu.0000:01:00.0.max_clock_mhz=3105"));
    let s = ctl.status_json();
    has(&s, &[
        r#""id":"0000:01:00.0","vendor":"nvidia","driver":"nvidia","name":"NVIDIA GeForce RTX 4090 Laptop GPU","integrated":false,"awake":true,"tempC":61"#,
        r#""clock":{"minMhz":600,"maxMhz":3105,"stepMhz":15,"limitMhz":null}"#,
        r#""power":null"#, // locked on this laptop: the test write was refused
        r#""level":null"#,
        r#""dynamicBoost":{"on":false}"#,
    ]);
}

#[test]
fn desktop_nvidia_power_limit_is_offered_once_verified() {
    let f = Fake::gt77_with_gpu(true);
    let run = FakeRun::default();
    *run.query_reply.lock().unwrap() = Some("250.00, 250.00, 100.00, 300.00\n".into());
    *run.pl_reply.lock().unwrap() = Some("Power limit for GPU 00000000:01:00.0 was set to 250.00 W from 250.00 W.\nAll done.\n".into());
    let mut ctl = gpu_ctl(&f, &run);
    assert!(run.calls().contains(&format!("{NVIDIA_SMI} -i {NV} -pl 250")), "verified with the current value");
    has(&ctl.status_json(), &[r#""power":{"minW":100,"maxW":300,"defaultW":250,"limitW":null}"#]);
    assert!(ctl.set_gpu_power_limit(NV, Some(350)).is_err());
    run.clear();
    ctl.set_gpu_power_limit(NV, Some(200)).unwrap();
    assert_eq!(run.calls(), vec![format!("{NVIDIA_SMI} -i {NV} -pl 200")]);
    assert!(f.get(SETTINGS_FILE).contains("gpu.0000:01:00.0.power_w=200"));
    run.clear();
    ctl.reset_advanced().unwrap();
    assert!(run.calls().contains(&format!("{NVIDIA_SMI} -i {NV} -pl 250")), "reset to default: {:?}", run.calls());
}

#[test]
fn nvidia_refusing_with_exit_code_zero_is_still_a_refusal() {
    let f = Fake::gt77_with_gpu(true);
    let run = FakeRun::default();
    *run.query_reply.lock().unwrap() = Some("150.00, 150.00, 5.00, 175.00\n".into()); // looks adjustable…
    let mut ctl = gpu_ctl(&f, &run); // …but -pl answers "not supported … All done."
    has(&ctl.status_json(), &[r#""power":null"#]);
    assert!(ctl.set_gpu_power_limit(NV, Some(140)).is_err());
}

#[test]
fn a_sleeping_gpu_is_never_touched_and_gets_its_settings_on_wake() {
    let f = Fake::gt77_with_gpu(true);
    let run = FakeRun::default();
    let mut ctl = gpu_ctl(&f, &run);
    f.gpu_awake(false);
    ctl.gpu_watch();
    run.clear();
    std::thread::sleep(std::time::Duration::from_millis(1600)); // let the cache expire
    let s = ctl.status_json();
    assert_eq!(run.smi_calls(), 0, "nvidia-smi must not run while the GPU sleeps: {:?}", run.calls());
    has(&s, &[r#""awake":false"#, r#""name":"NVIDIA GeForce RTX 4090 Laptop GPU""#]);
    ctl.set_gpu_clock_limit(NV, Some(2100)).unwrap();
    assert_eq!(run.smi_calls(), 0, "saved, not applied, while asleep");
    ctl.gpu_watch();
    assert_eq!(run.smi_calls(), 0, "still asleep");
    f.gpu_awake(true);
    let log = ctl.gpu_watch();
    assert!(run.calls().contains(&format!("{NVIDIA_SMI} -i {NV} -lgc 0,2100")), "{:?}", run.calls());
    assert!(log.iter().any(|l| l.contains("clock limit 2100 MHz applied")), "{log:?}");
    run.clear();
    ctl.gpu_watch();
    assert_eq!(run.smi_calls(), 0, "only on a wake, not every tick");
    ctl.redetect(); // after resume from sleep
    ctl.gpu_watch();
    assert!(run.calls().contains(&format!("{NVIDIA_SMI} -i {NV} -lgc 0,2100")));
}

#[test]
fn nvidia_clock_limit_is_validated_and_top_means_no_limit() {
    let f = Fake::gt77_with_gpu(true);
    let run = FakeRun::default();
    let mut ctl = gpu_ctl(&f, &run);
    assert!(ctl.set_gpu_clock_limit(NV, Some(300)).is_err());
    assert!(ctl.set_gpu_clock_limit(NV, Some(3200)).is_err());
    assert!(ctl.set_gpu_clock_limit("0000:09:00.0", Some(1800)).is_err());
    run.clear();
    ctl.set_gpu_clock_limit(NV, Some(1800)).unwrap();
    assert_eq!(run.calls(), vec![format!("{NVIDIA_SMI} -i {NV} -lgc 0,1800")]);
    run.clear();
    ctl.set_gpu_clock_limit(NV, Some(3105)).unwrap();
    assert_eq!(run.calls(), vec![format!("{NVIDIA_SMI} -i {NV} -rgc")]);
    assert!(ctl.settings().gpus.is_empty());
}

#[test]
fn v016_clock_limit_moves_onto_the_nvidia_gpu() {
    let f = Fake::gt77_with_gpu(true);
    f.put(SETTINGS_FILE, "gpu_clock_limit_mhz=1500\n").put(FACTORY_FILE, "gpu_max_clock_mhz=3105\n");
    let run = FakeRun::default();
    let ctl = gpu_ctl(&f, &run);
    assert_eq!(ctl.settings().gpus.get(NV).and_then(|g| g.clock_mhz), Some(1500));
    assert!(run.calls().contains(&format!("{NVIDIA_SMI} -i {NV} -lgc 0,1500")));
    assert!(f.get(SETTINGS_FILE).contains("gpu.0000:01:00.0.clock_mhz=1500"));
    assert!(!f.get(SETTINGS_FILE).contains("gpu_clock_limit_mhz"));
    assert!(f.get(FACTORY_FILE).contains("gpu.0000:01:00.0.max_clock_mhz=3105"));
}

#[test]
fn dynamic_boost_starts_stops_restores_and_resets() {
    let f = Fake::gt77_with_gpu(true);
    let run = FakeRun::default();
    let mut ctl = gpu_ctl(&f, &run);
    ctl.set_dynamic_boost(true).unwrap();
    assert!(*run.boost.lock().unwrap());
    has(&ctl.status_json(), &[r#""dynamicBoost":{"on":true}"#]);
    drop(ctl);
    *run.boost.lock().unwrap() = false; // reboot: not enabled at boot
    let mut ctl = gpu_ctl(&f, &run);
    assert_eq!(ctl.restore(), Vec::<String>::new());
    assert!(*run.boost.lock().unwrap());
    ctl.reset_advanced().unwrap();
    assert!(!*run.boost.lock().unwrap());
    assert_eq!(ctl.settings().dynamic_boost, None);
}

#[test]
fn dynamic_boost_needs_nvidia_powerd_and_an_nvidia_gpu() {
    let f = Fake::gt77_with_gpu(true);
    fs::remove_file(f.root.join("usr/lib/systemd/system/nvidia-powerd.service")).unwrap();
    let mut ctl = gpu_ctl(&f, &FakeRun::default());
    assert!(ctl.set_dynamic_boost(true).is_err());
    has(&ctl.status_json(), &[r#""dynamicBoost":null"#]);
}

fn amd_card(f: &Fake, pci: &str, vram: u64) -> String {
    let dev = f.gpu_dev(pci, "0x1002", "amdgpu");
    let h = format!("{dev}/hwmon/hwmon4");
    f.put(&format!("{dev}/mem_info_vram_total"), &format!("{vram}\n"))
        .put(&format!("{dev}/power_dpm_force_performance_level"), "auto\n")
        .put(&format!("{h}/name"), "amdgpu\n")
        .put(&format!("{h}/temp1_input"), "55000\n")
        .put(&format!("{h}/power1_average"), "120000000\n")
        .put(&format!("{h}/freq1_input"), "2400000000\n");
    h
}

#[test]
fn amd_radeon_power_limit_and_performance_level() {
    let f = Fake::new();
    let h = amd_card(&f, "0000:03:00.0", 16 << 30);
    f.put(&format!("{h}/power1_cap"), "200000000\n")
        .put(&format!("{h}/power1_cap_max"), "250000000\n")
        .put(&format!("{h}/power1_cap_min"), "100000000\n")
        .put(&format!("{h}/power1_cap_default"), "220000000\n")
        .put("/sys/bus/pci/devices/0000:03:00.0/product_name", "Radeon RX 7800 XT\n");
    let mut ctl = gpu_ctl(&f, &FakeRun::default());
    has(&ctl.status_json(), &[
        r#""vendor":"amd","driver":"amdgpu","name":"Radeon RX 7800 XT","integrated":false,"awake":true,"tempC":55,"powerW":120"#,
        r#""clockMhz":2400"#,
        r#""clock":null"#,
        r#""power":{"minW":100,"maxW":250,"defaultW":220,"limitW":null}"#,
        r#""level":{"options":["auto","low","high"],"current":"auto"}"#,
        r#""dynamicBoost":null"#,
    ]);
    ctl.set_gpu_power_limit("0000:03:00.0", Some(180)).unwrap();
    assert_eq!(f.get(&format!("{h}/power1_cap")), "180000000");
    ctl.set_gpu_level("0000:03:00.0", "high").unwrap();
    assert_eq!(f.get("/sys/bus/pci/devices/0000:03:00.0/power_dpm_force_performance_level"), "high");
    assert!(ctl.set_gpu_level("0000:03:00.0", "manual").is_err(), "only the three offered levels");
    ctl.reset_advanced().unwrap();
    assert_eq!(f.get(&format!("{h}/power1_cap")), "220000000");
    assert_eq!(f.get("/sys/bus/pci/devices/0000:03:00.0/power_dpm_force_performance_level"), "auto");
}

#[test]
fn amd_apu_alone_is_shown_and_offers_only_what_it_has() {
    let f = Fake::new();
    amd_card(&f, "0000:05:00.0", 512 << 20);
    let ctl = gpu_ctl(&f, &FakeRun::default());
    let shown = ctl.shown_gpus();
    assert_eq!(shown.len(), 1);
    assert!(shown[0].integrated);
    has(&ctl.status_json(), &[r#""name":"AMD Radeon Graphics""#, r#""power":null"#, r#""level":{"#]);
}

#[test]
fn intel_only_laptop_gets_a_gpu_clock_cap() {
    let f = Fake::new();
    f.intel_igpu();
    let mut ctl = gpu_ctl(&f, &FakeRun::default());
    let s = ctl.status_json();
    has(&s, &[
        r#""vendor":"intel","driver":"i915","name":"Intel Graphics","integrated":true"#,
        r#""clockMhz":450"#,
        r#""clock":{"minMhz":300,"maxMhz":1300,"stepMhz":50,"limitMhz":null}"#,
        r#""power":null"#,
    ]);
    ctl.set_gpu_clock_limit("0000:00:02.0", Some(900)).unwrap();
    assert_eq!(f.get("/sys/bus/pci/devices/0000:00:02.0/drm/card1/gt_max_freq_mhz"), "900");
    ctl.set_gpu_clock_limit("0000:00:02.0", None).unwrap();
    assert_eq!(f.get("/sys/bus/pci/devices/0000:00:02.0/drm/card1/gt_max_freq_mhz"), "1300");
}

#[test]
fn intel_arc_on_xe_gets_clock_and_power_controls() {
    let f = Fake::new();
    f.intel_igpu();
    let dev = f.gpu_dev("0000:03:00.0", "0x8086", "xe");
    let fq = format!("{dev}/tile0/gt0/freq0");
    f.put(&format!("{dev}/drm/card0/dev"), "226:0\n")
        .put(&format!("{fq}/rp0_freq"), "2400\n")
        .put(&format!("{fq}/rpn_freq"), "300\n")
        .put(&format!("{fq}/min_freq"), "300\n")
        .put(&format!("{fq}/max_freq"), "2400\n")
        .put(&format!("{fq}/act_freq"), "1000\n")
        .put(&format!("{dev}/hwmon/hwmon6/name"), "xe\n")
        .put(&format!("{dev}/hwmon/hwmon6/power1_max"), "190000000\n")
        .put(&format!("{dev}/hwmon/hwmon6/power1_rated_max"), "190000000\n");
    let mut ctl = gpu_ctl(&f, &FakeRun::default());
    let shown = ctl.shown_gpus();
    assert_eq!(shown.len(), 1, "Arc shown, built-in Intel hidden");
    let s = ctl.status_json();
    has(&s, &[
        r#""name":"Intel Arc GPU""#,
        r#""clock":{"minMhz":300,"maxMhz":2400,"stepMhz":50,"limitMhz":null}"#,
        r#""power":{"minW":95,"maxW":190,"defaultW":190,"limitW":null}"#,
    ]);
    ctl.set_gpu_clock_limit("0000:03:00.0", Some(1800)).unwrap();
    assert_eq!(f.get(&format!("{fq}/max_freq")), "1800");
    ctl.set_gpu_power_limit("0000:03:00.0", Some(150)).unwrap();
    assert_eq!(f.get(&format!("{dev}/hwmon/hwmon6/power1_max")), "150000000");
    assert!(ctl.set_gpu_power_limit("0000:03:00.0", Some(250)).is_err(), "never above rated");
}

#[test]
fn unsupported_drivers_are_listed_without_controls() {
    let f = Fake::new();
    f.gpu_dev("0000:01:00.0", "0x10de", "nouveau");
    let ctl = gpu_ctl(&f, &FakeRun::default());
    has(&ctl.status_json(), &[r#""driver":"nouveau""#, r#""clock":null,"power":null,"level":null"#]);
}

#[test]
fn no_gpus_at_all() {
    let f = Fake::new();
    let ctl = gpu_ctl(&f, &FakeRun::default());
    has(&ctl.status_json(), &[r#""gpus":[]"#, r#""dynamicBoost":null"#]);
}

// ---------- performance modes without msi-ec ----------

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn platform_profiles_from_different_makers_map_sensibly() {
    use crate::caps::map_profiles;
    let names = |v: Vec<(Mode, String)>| v.into_iter().map(|(m, n)| format!("{}={n}", m.id())).collect::<Vec<_>>();
    // ThinkPad
    assert_eq!(names(map_profiles(&words("low-power balanced performance"))),
        ["silent=low-power", "balanced=balanced", "performance=performance"]);
    // ASUS
    assert_eq!(names(map_profiles(&words("quiet balanced performance"))),
        ["silent=quiet", "balanced=balanced", "performance=performance"]);
    // Lenovo Legion
    assert_eq!(names(map_profiles(&words("quiet balanced balanced-performance performance custom"))),
        ["silent=quiet", "balanced=balanced", "performance=balanced-performance", "extreme=performance"]);
    // Dell
    assert_eq!(names(map_profiles(&words("cool quiet balanced performance"))),
        ["silent=quiet", "balanced=balanced", "performance=performance"]);
    // Something with max-power
    assert_eq!(names(map_profiles(&words("low-power balanced performance max-power"))),
        ["silent=low-power", "balanced=balanced", "performance=performance", "extreme=max-power"]);
    assert!(map_profiles(&words("custom")).is_empty());
}

#[test]
fn thinkpad_style_platform_profile_is_used_without_msi_ec() {
    let f = Fake::new();
    f.put("/sys/firmware/acpi/platform_profile", "balanced\n")
        .put("/sys/firmware/acpi/platform_profile_choices", "low-power balanced performance\n");
    let mut ctl = Controller::open_with(f.hw(), Box::new(FakeRun::default()));
    assert_eq!(ctl.caps().mode_backend, Some(crate::caps::ModeBackend::PlatformProfile));
    assert_eq!(ctl.live_mode(), Some(Mode::Balanced));
    ctl.set_mode(Mode::Silent).unwrap();
    assert_eq!(f.get("/sys/firmware/acpi/platform_profile"), "low-power");
    assert!(ctl.set_mode(Mode::Extreme).is_err());
    let s = ctl.status_json();
    assert!(s.contains(r#""modes":["silent","balanced","performance"],"modeBackend":"platform-profile""#), "{s}");
}

#[test]
fn power_profiles_daemon_is_preferred_over_writing_platform_profile() {
    let f = Fake::new();
    f.put("/sys/firmware/acpi/platform_profile", "balanced\n")
        .put("/sys/firmware/acpi/platform_profile_choices", "low-power balanced performance\n")
        .put("/usr/bin/powerprofilesctl", "");
    let run = FakeRun::default();
    *run.ppd.lock().unwrap() = Some("balanced".into());
    let mut ctl = Controller::open_with(f.hw(), Box::new(run.clone()));
    assert_eq!(ctl.caps().mode_backend, Some(crate::caps::ModeBackend::PowerProfilesDaemon));
    assert_eq!(ctl.live_mode(), Some(Mode::Balanced));
    ctl.set_mode(Mode::Performance).unwrap();
    assert_eq!(run.ppd.lock().unwrap().as_deref(), Some("performance"));
    assert_eq!(f.get("/sys/firmware/acpi/platform_profile"), "balanced", "file left to the daemon");
    assert_eq!(ctl.live_mode(), Some(Mode::Performance));
}

#[test]
fn stopped_power_profiles_daemon_falls_back_to_platform_profile() {
    let f = Fake::new();
    f.put("/sys/firmware/acpi/platform_profile", "quiet\n")
        .put("/sys/firmware/acpi/platform_profile_choices", "quiet balanced performance\n")
        .put("/usr/bin/powerprofilesctl", "");
    let ctl = Controller::open_with(f.hw(), Box::new(FakeRun::default())); // daemon not running
    assert_eq!(ctl.caps().mode_backend, Some(crate::caps::ModeBackend::PlatformProfile));
    assert_eq!(ctl.live_mode(), Some(Mode::Silent));
}

#[test]
fn msi_ec_wins_over_power_profiles_daemon() {
    let f = Fake::gt77();
    f.put("/usr/bin/powerprofilesctl", "");
    let run = FakeRun::default();
    *run.ppd.lock().unwrap() = Some("balanced".into());
    let ctl = Controller::open_with(f.hw(), Box::new(run));
    assert_eq!(ctl.caps().mode_backend, Some(crate::caps::ModeBackend::MsiEc));
}

// ---------- desktops and other laptops ----------

#[test]
fn desktop_without_msi_ec_uses_standard_sensors_and_has_no_battery() {
    let f = Fake::new();
    f.put("/sys/class/dmi/id/chassis_type", "3\n")
        .put("/sys/class/dmi/id/sys_vendor", "Micro-Star International Co., Ltd.\n")
        .put("/sys/class/hwmon/hwmon1/name", "coretemp\n")
        .put("/sys/class/hwmon/hwmon1/temp1_label", "Package id 0\n")
        .put("/sys/class/hwmon/hwmon1/temp1_input", "47000\n")
        .put("/sys/class/hwmon/hwmon1/temp2_label", "Core 0\n")
        .put("/sys/class/hwmon/hwmon1/temp2_input", "44000\n")
        // A wireless mouse battery must not count as the system battery.
        .put("/sys/class/power_supply/hidpp_battery_0/type", "Battery\n")
        .put("/sys/class/power_supply/hidpp_battery_0/scope", "Device\n")
        .put("/sys/class/power_supply/hidpp_battery_0/capacity", "70\n");
    let h = amd_card(&f, "0000:03:00.0", 16 << 30);
    f.put(&format!("{h}/power1_cap"), "200000000\n").put(&format!("{h}/power1_cap_max"), "250000000\n");
    let ctl = gpu_ctl(&f, &FakeRun::default());
    has(&ctl.status_json(), &[
        r#""formFactor":"desktop""#,
        r#""cpuTemp":47"#,
        r#""gpuTemp":55"#,
        r#""cpuFan":null"#,
        r#""present":false"#,
        r#""chargeLimit":false"#,
    ]);
}

#[test]
fn amd_cpu_temperature_prefers_tdie() {
    let f = Fake::new();
    f.put("/sys/class/hwmon/hwmon3/name", "k10temp\n")
        .put("/sys/class/hwmon/hwmon3/temp1_label", "Tctl\n")
        .put("/sys/class/hwmon/hwmon3/temp1_input", "71000\n")
        .put("/sys/class/hwmon/hwmon3/temp2_label", "Tdie\n")
        .put("/sys/class/hwmon/hwmon3/temp2_input", "61000\n");
    assert_eq!(crate::caps::generic_cpu_temp(&f.hw()), Some(61));
    f.put("/sys/class/hwmon/hwmon3/temp2_label", "Tccd1\n");
    assert_eq!(crate::caps::generic_cpu_temp(&f.hw()), Some(71));
}

#[test]
fn laptop_chassis_types_are_recognised() {
    let f = Fake::new();
    for (t, want) in [("10", "laptop"), ("31", "laptop"), ("9", "laptop"), ("3", "desktop"), ("35", "desktop"), ("1", "unknown")] {
        f.put("/sys/class/dmi/id/chassis_type", &format!("{t}\n"));
        assert_eq!(crate::caps::form_factor(&f.hw()), want, "chassis {t}");
    }
}

#[test]
fn system_report_summarises_detection() {
    let f = Fake::gt77_with_gpu(true);
    f.put("/proc/sys/kernel/osrelease", "7.3.5-2-cachyos\n");
    let ctl = gpu_ctl(&f, &FakeRun::default());
    let r = ctl.report();
    for needle in [
        "Kernel                7.3.5-2-cachyos",
        "msi-ec                yes, EC firmware 17Q2IMS1.10E",
        "Mode backend          msi-ec",
        "Fan curves            available",
        "GPU 0000:01:00.0      nvidia driver=nvidia integrated=false awake=true backend=nvidia power=None",
        "GPU 0000:00:02.0      intel driver=i915 integrated=true",
        "Status:",
    ] {
        assert!(r.contains(needle), "missing {needle:?} in\n{r}");
    }
    assert!(!r.to_lowercase().contains("serial"));
}

// ---------- MSI extras ----------

#[test]
fn msi_extras_are_detected_set_and_reported() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    has(&ctl.status_json(), &[r#""extras":{"webcam":true,"webcamBlock":false,"fnKey":"left","superBattery":false,"kbdBacklight":null}"#]);
    ctl.set_super_battery(true).unwrap();
    assert_eq!(f.get(&format!("{EC}/super_battery")), "on");
    ctl.set_fn_key("right").unwrap();
    assert_eq!(f.get(&format!("{EC}/fn_key")), "right");
    assert!(ctl.set_fn_key("middle").is_err());
    ctl.set_webcam(false).unwrap();
    assert_eq!(f.get(&format!("{EC}/webcam")), "off");
}

#[test]
fn blocking_the_webcam_switches_it_off_and_keeps_it_off() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_webcam_block(true).unwrap();
    assert_eq!(f.get(&format!("{EC}/webcam")), "off");
    assert_eq!(f.get(&format!("{EC}/webcam_block")), "on");
    let err = ctl.set_webcam(true).unwrap_err();
    assert!(err.0.contains("blocked"), "{err}");
    ctl.set_webcam_block(false).unwrap();
    ctl.set_webcam(true).unwrap();
    assert_eq!(f.get(&format!("{EC}/webcam")), "on");
}

#[test]
fn msi_extras_come_back_after_a_reboot() {
    let f = Fake::gt77();
    let mut ctl = Controller::open(f.hw());
    ctl.set_webcam_block(true).unwrap();
    ctl.set_fn_key("right").unwrap();
    ctl.set_super_battery(true).unwrap();
    drop(ctl);
    f.put(&format!("{EC}/webcam"), "on\n")
        .put(&format!("{EC}/webcam_block"), "off\n")
        .put(&format!("{EC}/fn_key"), "left\n")
        .put(&format!("{EC}/super_battery"), "off\n");
    assert_eq!(Controller::open(f.hw()).restore(), Vec::<String>::new());
    assert_eq!(f.get(&format!("{EC}/webcam")), "off");
    assert_eq!(f.get(&format!("{EC}/webcam_block")), "on");
    assert_eq!(f.get(&format!("{EC}/fn_key")), "right");
    assert_eq!(f.get(&format!("{EC}/super_battery")), "on");
}

#[test]
fn laptops_without_the_extras_show_none() {
    let f = Fake::new();
    let mut ctl = Controller::open(f.hw());
    has(&ctl.status_json(), &[r#""extras":{"webcam":null,"webcamBlock":null,"fnKey":null,"superBattery":null,"kbdBacklight":null}"#]);
    assert!(ctl.set_webcam(false).is_err());
    assert!(ctl.set_super_battery(true).is_err());
}

#[test]
fn keyboard_backlight_prefers_msi_and_respects_its_levels() {
    let f = Fake::gt77();
    f.put("/sys/class/leds/msiacpi::kbd_backlight/max_brightness", "3\n")
        .put("/sys/class/leds/msiacpi::kbd_backlight/brightness", "1\n")
        .put("/sys/class/leds/input3::capslock/max_brightness", "1\n");
    let mut ctl = Controller::open(f.hw());
    has(&ctl.status_json(), &[r#""kbdBacklight":{"level":1,"max":3}"#]);
    ctl.set_kbd_backlight(3).unwrap();
    assert_eq!(f.get("/sys/class/leds/msiacpi::kbd_backlight/brightness"), "3");
    assert!(ctl.set_kbd_backlight(4).is_err());
    // Other laptops' backlights work the same way.
    let g = Fake::new();
    g.put("/sys/class/leds/tpacpi::kbd_backlight/max_brightness", "2\n")
        .put("/sys/class/leds/tpacpi::kbd_backlight/brightness", "0\n");
    let mut ctl = Controller::open(g.hw());
    ctl.set_kbd_backlight(2).unwrap();
    assert_eq!(g.get("/sys/class/leds/tpacpi::kbd_backlight/brightness"), "2");
}


// ---------- fan test on other MSI models ----------

const SWORD_FW: &str = "15P1IMS1.10A";

/// A Sword 16 HX with the common MSI fan-table layout, not yet verified.
fn sword() -> (Fake, FakeRun) {
    let f = Fake::gt77();
    f.put("/sys/class/dmi/id/product_name", "Sword 16 HX B14VGKG\n")
        .put("/sys/class/dmi/id/board_name", "MS-15P1\n")
        .put(&format!("{EC}/fw_version"), &format!("{SWORD_FW}\n"))
        .put("/usr/bin/modprobe", "");
    (f, FakeRun::default())
}

fn open_fast(f: &Fake, run: &FakeRun) -> Controller {
    let mut ctl = Controller::open_with(f.hw(), Box::new(run.clone()));
    ctl.set_fan_test_timing(std::time::Duration::from_millis(150), std::time::Duration::ZERO);
    ctl
}

#[test]
fn fan_test_verifies_both_curves_and_restores_everything() {
    let (f, run) = sword();
    let mut ctl = open_fast(&f, &run);
    assert!(!ctl.caps().fan_curves);
    assert!(ctl.fan_test_available());
    assert!(ctl.status_json().contains(r#""fanTest":{"available":true,"running":false"#));
    assert!(ctl.set_fan_preset(Preset::Cool).is_err(), "no writes before the test");

    ctl.start_fan_test().unwrap();
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap().cpu.speeds, [100; 7]);
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "advanced");
    assert!(ctl.set_fan_preset(Preset::Cool).is_err(), "busy while testing");
    assert!(ctl.fan_test_tick().is_empty(), "fan still slow");

    f.put(&format!("{EC}/cpu/realtime_fan_speed"), "96\n");
    assert!(ctl.fan_test_tick()[0].contains("CPU"));
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), Tables { cpu: CPU, gpu: GPU }, "CPU table back before the GPU step");
    f.put(&format!("{EC}/cpu/realtime_fan_speed"), "40\n");
    ctl.fan_test_tick(); // settle → GPU step
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap().gpu.speeds, [100; 7]);
    f.put(&format!("{EC}/gpu/realtime_fan_speed"), "92\n");
    assert!(ctl.fan_test_tick()[0].contains("CPU and GPU"));

    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), Tables { cpu: CPU, gpu: GPU });
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "auto");
    assert!(ctl.caps().fan_curves, "{:?}", ctl.caps().fan_curve_reason);
    assert!(ctl.fan_layout().gpu);
    assert!(ctl.status_json().contains(r#""ok":true"#));

    // Remembered across restarts, for this firmware only.
    let ctl = open_fast(&f, &run);
    assert!(ctl.caps().fan_curves);
    f.put(&format!("{EC}/fw_version"), "15P1IMS1.10B\n");
    let ctl = open_fast(&f, &run);
    assert!(!ctl.caps().fan_curves, "new EC firmware needs a new test");
}

#[test]
fn single_curve_boards_never_touch_the_gpu_block() {
    let (f, run) = sword();
    let mut ctl = open_fast(&f, &run);
    ctl.start_fan_test().unwrap();
    f.put(&format!("{EC}/cpu/realtime_fan_speed"), "96\n");
    ctl.fan_test_tick();
    f.put(&format!("{EC}/cpu/realtime_fan_speed"), "40\n");
    ctl.fan_test_tick();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(ctl.fan_test_tick()[0].contains("single curve"));
    assert!(ctl.caps().fan_curves);
    assert!(!ctl.fan_layout().gpu);
    assert!(ctl.status_json().contains(r#""gpu":null"#));

    let mut curve = Tables { cpu: CPU, gpu: CPU };
    curve.cpu.speeds = [40, 50, 60, 70, 80, 90, 100];
    curve.cpu.temps = [50, 60, 70, 80, 85, 90, 100];
    ctl.set_custom_curve(curve).unwrap();
    let live = fan::read(&f.hw(), fan::FULL).unwrap();
    assert_eq!(live.cpu, curve.cpu);
    assert_eq!(live.gpu, GPU, "unused GPU block left alone");
}

#[test]
fn fan_test_that_gets_no_response_keeps_curves_off() {
    let (f, run) = sword();
    let mut ctl = open_fast(&f, &run);
    ctl.start_fan_test().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(ctl.fan_test_tick()[0].contains("failed"));
    assert!(!ctl.caps().fan_curves);
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), Tables { cpu: CPU, gpu: GPU });
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "auto");
    assert!(ctl.status_json().contains(r#""ok":false"#));
}

#[test]
fn fan_test_refuses_odd_tables_busy_fans_and_cancels_cleanly() {
    let (f, run) = sword();
    f.put(&format!("{EC}/cpu/realtime_fan_speed"), "85\n");
    let mut ctl = open_fast(&f, &run);
    assert!(ctl.start_fan_test().unwrap_err().to_string().contains("idle"));

    f.put(&format!("{EC}/cpu/realtime_fan_speed"), "40\n");
    ctl.start_fan_test().unwrap();
    ctl.cancel_fan_test();
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), Tables { cpu: CPU, gpu: GPU });
    assert!(!ctl.caps().fan_curves);

    let (f, run) = sword();
    let mut odd = Tables { cpu: CPU, gpu: GPU };
    odd.cpu.temps = [0, 0, 0, 0, 0, 0, 0];
    f.ec_bytes(&odd);
    let ctl = open_fast(&f, &run);
    assert!(!ctl.fan_test_available(), "a block that isn't a fan curve is never tested");
    assert!(ctl.report().contains("EC fan area"));
}

#[test]
fn the_gt77_never_offers_the_fan_test() {
    let ctl = Controller::open(Fake::gt77().hw());
    assert!(!ctl.fan_test_available());
}

#[test]
fn an_interrupted_fan_test_is_undone_at_the_next_start() {
    let (f, run) = sword();
    let mut ctl = open_fast(&f, &run);
    ctl.start_fan_test().unwrap();
    drop(ctl); // service killed mid-test
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap().cpu.speeds, [100; 7]);
    let ctl = open_fast(&f, &run);
    assert_eq!(fan::read(&f.hw(), fan::FULL).unwrap(), Tables { cpu: CPU, gpu: GPU });
    assert_eq!(f.get(&format!("{EC}/fan_mode")), "auto");
    assert!(!ctl.caps().fan_curves);
}
