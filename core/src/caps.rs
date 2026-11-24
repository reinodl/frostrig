//! Works out what this laptop supports. Nothing here writes to hardware.

use crate::hw::Hw;
use crate::Mode;

pub const MSI_EC: &str = "/sys/devices/platform/msi-ec";
pub const DMI: &str = "/sys/class/dmi/id";
pub const POWER_SUPPLY: &str = "/sys/class/power_supply";
pub const POWERCAP: &str = "/sys/class/powercap";
pub const INTEL_NO_TURBO: &str = "/sys/devices/system/cpu/intel_pstate/no_turbo";
pub const CPUFREQ_BOOST: &str = "/sys/devices/system/cpu/cpufreq/boost";
pub const SCALING_DRIVER: &str = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver";
pub const EC_IO: &str = "/sys/kernel/debug/ec/ec0/io";
pub const EC_WRITE_SUPPORT: &str = "/sys/module/ec_sys/parameters/write_support";
pub const MODPROBE: &str = "/usr/bin/modprobe";

/// The one laptop whose raw EC fan tables have been verified. Fan curves are
/// refused on everything else, however similar it looks.
pub const GT77_MODEL: &str = "Titan GT77HX 13VI";
pub const GT77_BOARD: &str = "MS-17Q2";
pub const GT77_EC_FW: &str = "17Q2IMS1.10E";

pub const PLATFORM_PROFILE: &str = "/sys/firmware/acpi/platform_profile";
pub const PLATFORM_PROFILE_CHOICES: &str = "/sys/firmware/acpi/platform_profile_choices";
pub const POWERPROFILESCTL: &str = "/usr/bin/powerprofilesctl";

/// Where performance modes come from, in order of preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeBackend {
    /// MSI laptops: msi-ec shift modes (plus its fan modes).
    MsiEc,
    /// power-profiles-daemon (or tuned-ppd) is running: go through it, so
    /// the desktop's own power slider and FrostRig never fight.
    PowerProfilesDaemon,
    /// The kernel's ACPI platform profile: ThinkPad, ASUS, Dell, HP, Lenovo, ...
    PlatformProfile,
}

impl ModeBackend {
    pub fn id(self) -> &'static str {
        match self {
            ModeBackend::MsiEc => "msi-ec",
            ModeBackend::PowerProfilesDaemon => "power-profiles-daemon",
            ModeBackend::PlatformProfile => "platform-profile",
        }
    }
}

/// Map a list of profile names (platform_profile choices or power-profiles-daemon
/// profiles) onto FrostRig's four modes. Each name is used at most once and
/// modes with no matching profile are left out.
pub fn map_profiles(choices: &[String]) -> Vec<(Mode, String)> {
    let has = |n: &str| choices.iter().any(|c| c == n);
    let pick = |names: &[&str], not: Option<&str>| {
        names.iter().find(|n| has(n) && Some(**n) != not).map(|n| (*n).to_owned())
    };
    let silent = pick(&["quiet", "low-power", "power-saver", "cool"], None);
    let balanced = pick(&["balanced"], None);
    let performance = pick(&["balanced-performance", "performance"], None);
    let extreme = pick(&["max-power", "performance"], performance.as_deref());
    [(Mode::Silent, silent), (Mode::Balanced, balanced), (Mode::Performance, performance), (Mode::Extreme, extreme)]
        .into_iter()
        .filter_map(|(m, n)| n.map(|n| (m, n)))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurboSwitch {
    /// intel_pstate: `no_turbo`, where "1" means Turbo is OFF.
    IntelNoTurbo,
    /// generic cpufreq: `boost`, where "1" means Turbo is ON.
    CpufreqBoost,
}

#[derive(Clone, Debug, Default)]
pub struct Caps {
    pub model: Option<String>,
    pub board: Option<String>,
    pub ec_firmware: Option<String>,
    pub msi_ec: bool,
    /// Supported modes, each with the backend's own name for it.
    pub modes: Vec<(Mode, String)>,
    pub mode_backend: Option<ModeBackend>,
    pub fan_modes: Vec<String>,
    pub cooler_boost: bool,
    /// msi-ec extras, each true when its attribute exists.
    pub webcam: bool,
    pub webcam_block: bool,
    pub fn_key: bool,
    pub super_battery: bool,
    /// Keyboard backlight LED directory and its maximum level (any driver).
    pub kbd_backlight: Option<(String, u64)>,
    /// Battery directory with a writable end-of-charge threshold.
    pub battery: Option<String>,
    pub battery_linked_start: bool,
    pub turbo: Option<TurboSwitch>,
    /// RAPL package zone, e.g. /sys/class/powercap/intel-rapl:0
    pub rapl: Option<String>,
    pub fan_curves: bool,
    /// The verified Titan GT77HX 13VI: fan curves without a fan test.
    pub fan_builtin: bool,
    /// Why fan curves are unavailable, for the app to show.
    pub fan_curve_reason: Option<String>,
}

impl Caps {
    pub fn is_gt77(&self) -> bool {
        self.model.as_deref() == Some(GT77_MODEL)
            && self.board.as_deref() == Some(GT77_BOARD)
            && self.ec_firmware.as_deref() == Some(GT77_EC_FW)
    }

    pub fn shift_for(&self, mode: Mode) -> Option<&str> {
        self.modes.iter().find(|(m, _)| *m == mode).map(|(_, s)| s.as_str())
    }

    pub fn has_fan_mode(&self, name: &str) -> bool {
        self.fan_modes.iter().any(|m| m == name)
    }
}

/// A laptop's own battery, not a wireless mouse or headset (those report scope "Device").
pub fn is_system_battery(hw: &Hw, dir: &str) -> bool {
    hw.read(&format!("{dir}/type")).as_deref() == Some("Battery")
        && hw.read(&format!("{dir}/scope")).as_deref() != Some("Device")
}

/// "laptop", "desktop" or "unknown", from the SMBIOS chassis type.
pub fn form_factor(hw: &Hw) -> &'static str {
    match hw.read_u64(&format!("{DMI}/chassis_type")) {
        Some(8 | 9 | 10 | 11 | 14 | 30 | 31 | 32) => "laptop",
        Some(3..=7 | 13 | 15 | 16 | 17 | 23 | 24 | 35 | 36) => "desktop",
        _ => "unknown",
    }
}

/// CPU package temperature from the standard Linux sensors, for machines
/// without msi-ec: Intel coretemp, AMD k10temp/zenpower, then ACPI.
pub fn generic_cpu_temp(hw: &Hw) -> Option<u64> {
    let devices: Vec<(String, String)> = hw.list_dir("/sys/class/hwmon").into_iter()
        .map(|d| format!("/sys/class/hwmon/{d}"))
        .filter_map(|d| Some((hw.read(&format!("{d}/name"))?, d)))
        .collect();
    let labelled = |dir: &str, wanted: &[&str]| -> Option<u64> {
        for w in wanted {
            for n in 1..=32 {
                if hw.read(&format!("{dir}/temp{n}_label")).as_deref() == Some(*w) {
                    return hw.read_u64(&format!("{dir}/temp{n}_input"));
                }
            }
        }
        None
    };
    let find = |name: &str| devices.iter().find(|(n, _)| n == name).map(|(_, d)| d.as_str());
    let milli = find("coretemp").and_then(|d| labelled(d, &["Package id 0"]))
        .or_else(|| find("k10temp").and_then(|d| labelled(d, &["Tdie", "Tctl"])))
        .or_else(|| find("zenpower").and_then(|d| labelled(d, &["Tdie", "Tctl"])))
        .or_else(|| find("cpu_thermal").and_then(|d| hw.read_u64(&format!("{d}/temp1_input"))))
        .or_else(|| find("acpitz").and_then(|d| hw.read_u64(&format!("{d}/temp1_input"))))?;
    Some(milli / 1000)
}

pub fn detect(hw: &Hw) -> Caps {
    let mut c = Caps {
        model: hw.read(&format!("{DMI}/product_name")),
        board: hw.read(&format!("{DMI}/board_name")),
        ec_firmware: hw.read(&format!("{MSI_EC}/fw_version")),
        msi_ec: hw.exists(MSI_EC),
        ..Caps::default()
    };

    if c.msi_ec {
        let shifts = hw.read_words(&format!("{MSI_EC}/available_shift_modes"));
        if hw.exists(&format!("{MSI_EC}/shift_mode")) {
            for mode in Mode::ALL {
                if let Some(name) = mode.shift_names().iter().find(|n| shifts.iter().any(|s| s == *n)) {
                    c.modes.push((mode, (*name).to_owned()));
                }
            }
        }
        if hw.exists(&format!("{MSI_EC}/fan_mode")) {
            c.fan_modes = hw.read_words(&format!("{MSI_EC}/available_fan_modes"));
        }
        c.cooler_boost = hw.exists(&format!("{MSI_EC}/cooler_boost"));
        c.webcam = hw.exists(&format!("{MSI_EC}/webcam"));
        c.webcam_block = hw.exists(&format!("{MSI_EC}/webcam_block"));
        c.fn_key = hw.exists(&format!("{MSI_EC}/fn_key"));
        c.super_battery = hw.exists(&format!("{MSI_EC}/super_battery"));
    }
    // Keyboard backlight: msi-ec names it msiacpi::kbd_backlight; other
    // drivers use the same "::kbd_backlight" suffix. RGB keyboards don't
    // appear here and are left alone.
    let mut leds = hw.list_dir("/sys/class/leds");
    leds.sort_by_key(|l| !l.starts_with("msiacpi"));
    c.kbd_backlight = leds.into_iter()
        .filter(|l| l.ends_with("::kbd_backlight"))
        .map(|l| format!("/sys/class/leds/{l}"))
        .find_map(|d| {
            let max = hw.read_u64(&format!("{d}/max_brightness"))?;
            (1..=10).contains(&max).then_some((d, max))
        });

    if !c.modes.is_empty() {
        c.mode_backend = Some(ModeBackend::MsiEc);
    } else if hw.exists(PLATFORM_PROFILE) {
        c.modes = map_profiles(&hw.read_words(PLATFORM_PROFILE_CHOICES));
        if !c.modes.is_empty() {
            c.mode_backend = Some(ModeBackend::PlatformProfile);
        }
    }

    // Battery: prefer BAT1 (MSI's usual name), then any battery.
    let mut names = hw.list_dir(POWER_SUPPLY);
    names.sort_by_key(|n| n != "BAT1");
    for name in names {
        let dir = format!("{POWER_SUPPLY}/{name}");
        if is_system_battery(hw, &dir)
            && hw.exists(&format!("{dir}/charge_control_end_threshold"))
        {
            // msi-ec drives both thresholds from one EC byte (start = end - 10),
            // so only the end threshold is ever written there.
            c.battery_linked_start = !c.msi_ec && hw.exists(&format!("{dir}/charge_control_start_threshold"));
            c.battery = Some(dir);
            break;
        }
    }

    let driver = hw.read(SCALING_DRIVER);
    if matches!(driver.as_deref(), Some("intel_pstate" | "intel_cpufreq")) && hw.exists(INTEL_NO_TURBO) {
        c.turbo = Some(TurboSwitch::IntelNoTurbo);
    } else if hw.exists(CPUFREQ_BOOST) {
        c.turbo = Some(TurboSwitch::CpufreqBoost);
    }

    for zone in hw.list_dir(POWERCAP) {
        if !zone.starts_with("intel-rapl:") || zone.matches(':').count() != 1 { continue; }
        let dir = format!("{POWERCAP}/{zone}");
        if hw.read(&format!("{dir}/name")).as_deref() == Some("package-0")
            && hw.read(&format!("{dir}/constraint_0_name")).as_deref() == Some("long_term")
            && hw.read(&format!("{dir}/constraint_1_name")).as_deref() == Some("short_term")
        {
            c.rapl = Some(dir);
            break;
        }
    }

    // Whether fan curves are usable also depends on the on-device fan test
    // recorded in the factory file, so the controller decides (refresh_fan).
    c.fan_builtin = c.is_gt77();
    c
}
