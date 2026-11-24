//! Applies the user's choices with read-back checks, remembers them, and
//! puts them back at boot and after sleep.

use crate::caps::{self, Caps, ModeBackend, TurboSwitch, MSI_EC, PLATFORM_PROFILE, POWERPROFILESCTL, POWER_SUPPLY};
use crate::cpu;
use crate::fan::{self, Preset, Tables};
use crate::gpu::{self, Gpu, Runner, SystemRunner, Vendor};
use crate::state::GpuSettings;
use std::collections::BTreeMap;
use crate::hw::Hw;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use crate::json::J;
use crate::state::{Factory, Settings, FACTORY_FILE, SETTINGS_FILE};
use crate::{Error, Mode, Result, CHARGE_LIMITS, VERSION};

/// Lowest power limits the app will set, whatever the laptop.
pub const PL1_MIN_W: u32 = 15;
pub const PL2_MIN_W: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerLimits {
    pub pl1_min: u32,
    pub pl1_max: u32,
    pub pl2_min: u32,
    pub pl2_max: u32,
}

/// Live GPU readings are cached briefly so status polls don't run
/// nvidia-smi more than needed.
const GPU_CACHE: Duration = Duration::from_millis(1500);

/// A CPU power reading older than this is stale (the sampler stopped).
const CPU_POWER_FRESH: Duration = Duration::from_secs(6);

/// One reading of the RAPL energy counter and the watts worked out from the
/// reading before it.
#[derive(Clone, Copy, Debug)]
struct EnergySample {
    at: Instant,
    uj: u64,
    watts: Option<f64>,
}

const FAN_TEST_FILE: &str = "/var/lib/frostrig/fan-test";

/// Fan test timings: how long to wait for a fan to speed up, and to settle between fans.
const FAN_TEST_WAIT: Duration = Duration::from_secs(25);
const FAN_TEST_SETTLE: Duration = Duration::from_secs(10);
/// A fan counts as responding when it rises this much (percent) or passes FAN_TEST_FAST.
const FAN_TEST_RISE: u64 = 20;
const FAN_TEST_FAST: u64 = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestPhase {
    Cpu,
    Settle,
    Gpu,
}

/// The on-device fan test that unlocks fan curves on MSI models other than
/// the built-in verified one. It only ever raises fan speed (the safe
/// direction) and always puts the original tables back.
#[derive(Clone, Debug)]
enum FanTest {
    Idle,
    Running {
        phase: TestPhase,
        since: Instant,
        before: Tables,
        mode_before: Option<String>,
        baseline: u64,
        gpu_candidate: bool,
    },
    Done { ok: bool, message: String },
}

/// What was learned about a GPU the last time it woke up.
#[derive(Clone, Debug, Default)]
struct GpuProbe {
    name: Option<String>,
    /// Some only if the driver accepted a test write of the current limit.
    power: Option<gpu::PowerRange>,
    power_checked: bool,
    intel_clock: Option<gpu::ClockRange>,
}

pub struct Controller {
    hw: Hw,
    caps: Caps,
    settings: Settings,
    factory: Factory,
    run: Box<dyn Runner>,
    gpus: Vec<Gpu>,
    gpu_was_awake: BTreeMap<String, bool>,
    gpu_probe: BTreeMap<String, GpuProbe>,
    gpu_live: Mutex<BTreeMap<String, (Instant, gpu::Live)>>,
    boost_cache: Mutex<Option<(Instant, bool)>>,
    /// power-profiles-daemon's current profile, cached like GPU readings.
    ppd_cache: Mutex<Option<(Instant, Option<String>)>>,
    fan_test: FanTest,
    cpu_energy: Option<EnergySample>,
    /// (wait, settle) for the fan test; shortened in tests.
    fan_test_timing: (Duration, Duration),
}

impl Controller {
    /// Detect hardware, load saved settings, and record factory defaults the
    /// first time (before anything is changed).
    pub fn open(hw: Hw) -> Controller {
        Controller::open_with(hw, Box::new(SystemRunner))
    }

    /// As `open`, with a different way to run programs (tests use a fake).
    pub fn open_with(hw: Hw, run: Box<dyn Runner>) -> Controller {
        let caps = caps::detect(&hw);
        let settings = hw.read(SETTINGS_FILE).map(|t| Settings::parse(&t)).unwrap_or_default();
        let factory = hw.read(FACTORY_FILE).map(|t| Factory::parse(&t)).unwrap_or_default();
        let gpus = gpu::detect_all(&hw);
        let mut c = Controller {
            hw, caps, settings, factory, run, gpus,
            gpu_was_awake: BTreeMap::new(),
            gpu_probe: BTreeMap::new(),
            gpu_live: Mutex::new(BTreeMap::new()),
            boost_cache: Mutex::new(None),
            ppd_cache: Mutex::new(None),
            fan_test: FanTest::Idle,
            cpu_energy: None,
            fan_test_timing: (FAN_TEST_WAIT, FAN_TEST_SETTLE),
        };
        c.detect_ppd();
        c.prepare_ec();
        c.recover_fan_test();
        c.refresh_fan();
        c.migrate_legacy_gpu();
        c.capture_factory();
        c
    }

    /// Re-detect, e.g. once kernel modules have finished loading at boot or
    /// after resume. GPU settings are re-applied on the next `gpu_watch`.
    pub fn redetect(&mut self) {
        self.caps = caps::detect(&self.hw);
        self.gpus = gpu::detect_all(&self.hw);
        self.gpu_was_awake.clear();
        self.invalidate_gpu();
        self.detect_ppd();
        self.prepare_ec();
        self.refresh_fan();
        self.capture_factory();
    }

    /// Without msi-ec, prefer power-profiles-daemon when it's running: it owns
    /// platform_profile, so writing that file directly would fight it.
    fn detect_ppd(&mut self) {
        if self.caps.mode_backend == Some(ModeBackend::MsiEc) || !self.hw.exists(POWERPROFILESCTL) {
            return;
        }
        let Ok(out) = self.run.run(POWERPROFILESCTL, &["list"]) else { return };
        // Lines look like "* performance:" or "  balanced:"; details lines don't end in ':'.
        let names: Vec<String> = out.lines()
            .map(|l| l.trim().trim_start_matches('*').trim())
            .filter_map(|l| l.strip_suffix(':'))
            .filter(|n| !n.is_empty() && !n.contains(' '))
            .map(str::to_owned)
            .collect();
        let modes = caps::map_profiles(&names);
        if !modes.is_empty() {
            self.caps.modes = modes;
            self.caps.mode_backend = Some(ModeBackend::PowerProfilesDaemon);
        }
    }

    fn ppd_current(&self) -> Option<String> {
        let mut cache = self.ppd_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, v)) = cache.as_ref() {
            if at.elapsed() < GPU_CACHE { return v.clone(); }
        }
        let v = self.run.run(POWERPROFILESCTL, &["get"]).ok().map(|s| s.trim().to_owned());
        *cache = Some((Instant::now(), v.clone()));
        v
    }

    pub fn caps(&self) -> &Caps {
        &self.caps
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Fill in any factory value we don't have yet, but only for features the
    /// user hasn't changed through us; otherwise we'd record their setting.
    fn capture_factory(&mut self) {
        let before = self.factory.clone();
        if let Some(z) = self.caps.rapl.clone() {
            if self.settings.cpu_power.is_none() && self.factory.pl1_uw.is_none() {
                self.factory.pl1_uw = self.hw.read_u64(&format!("{z}/constraint_0_power_limit_uw"));
                self.factory.pl2_uw = self.hw.read_u64(&format!("{z}/constraint_1_power_limit_uw"));
            }
        }
        if self.settings.turbo.is_none() && self.factory.turbo.is_none() {
            self.factory.turbo = self.live_turbo();
        }
        if self.caps.fan_curves
            && self.factory.fan_tables.is_none()
            && matches!(self.settings.fan_preset, None | Some(Preset::Auto))
        {
            if let Ok(t) = fan::read(&self.hw, self.fan_layout()) {
                if fan::plausible(&t, self.fan_layout()).is_ok() {
                    self.factory.fan_tables = Some(t);
                }
            }
        }
        if self.factory != before {
            // Best effort; if this fails we'll try again next start.
            let _ = self.hw.save_private(FACTORY_FILE, &self.factory.serialize());
        }
    }

    fn save(&self) -> Result<()> {
        self.hw.save_private(SETTINGS_FILE, &self.settings.serialize())
    }

    // ---------- live readings ----------

    fn ec(&self, name: &str) -> String {
        format!("{MSI_EC}/{name}")
    }

    pub fn live_mode(&self) -> Option<Mode> {
        let name = match self.caps.mode_backend? {
            ModeBackend::MsiEc => self.hw.read(&self.ec("shift_mode"))?,
            ModeBackend::PlatformProfile => self.hw.read(PLATFORM_PROFILE)?,
            ModeBackend::PowerProfilesDaemon => self.ppd_current()?,
        };
        self.caps.modes.iter().find(|(_, s)| *s == name).map(|(m, _)| *m)
    }

    fn live_turbo(&self) -> Option<bool> {
        match self.caps.turbo? {
            TurboSwitch::IntelNoTurbo => self.hw.read(caps::INTEL_NO_TURBO).map(|v| v == "0"),
            TurboSwitch::CpufreqBoost => self.hw.read(caps::CPUFREQ_BOOST).map(|v| v == "1"),
        }
    }

    fn live_power_w(&self) -> Option<(f64, f64)> {
        let z = self.caps.rapl.as_ref()?;
        let a = self.hw.read_u64(&format!("{z}/constraint_0_power_limit_uw"))?;
        let b = self.hw.read_u64(&format!("{z}/constraint_1_power_limit_uw"))?;
        Some((a as f64 / 1e6, b as f64 / 1e6))
    }

    fn live_charge_limit(&self) -> Option<u64> {
        self.hw.read_u64(&format!("{}/charge_control_end_threshold", self.caps.battery.as_ref()?))
    }

    /// Slider ranges for CPU power. The top is the laptop's own default:
    /// the app can lower power, never raise it past what the firmware set.
    pub fn power_limits(&self) -> Option<PowerLimits> {
        let z = self.caps.rapl.as_ref()?;
        let w = |uw: u64| (uw / 1_000_000) as u32;
        let pl1 = self.factory.pl1_uw.or_else(|| self.hw.read_u64(&format!("{z}/constraint_0_power_limit_uw")))?;
        let pl2 = self.factory.pl2_uw.or_else(|| self.hw.read_u64(&format!("{z}/constraint_1_power_limit_uw")))?;
        let pl1_max = w(pl1).max(PL1_MIN_W);
        let pl2_max = w(pl2).max(pl1_max).max(PL2_MIN_W);
        Some(PowerLimits { pl1_min: PL1_MIN_W, pl1_max, pl2_min: PL2_MIN_W, pl2_max })
    }

    fn preset(&self) -> Preset {
        self.settings.fan_preset.unwrap_or(Preset::Auto)
    }

    // ---------- apply steps (no saving) ----------

    fn fan_mode_for(&self, mode: Option<Mode>, preset: Preset) -> Option<&'static str> {
        if preset != Preset::Auto && self.caps.fan_curves && self.caps.has_fan_mode("advanced") {
            Some("advanced")
        } else if mode == Some(Mode::Silent) && self.caps.has_fan_mode("silent") {
            Some("silent")
        } else if self.caps.has_fan_mode("auto") {
            Some("auto")
        } else {
            None
        }
    }

    fn sync_fan_mode(&self, mode: Option<Mode>, preset: Preset) -> Result<()> {
        if let Some(want) = self.fan_mode_for(mode, preset) {
            if self.hw.read(&self.ec("fan_mode")).as_deref() != Some(want) {
                self.hw.write_verified(&self.ec("fan_mode"), want)?;
            }
        }
        Ok(())
    }

    fn apply_mode(&self, mode: Mode) -> Result<()> {
        let name = self.caps.shift_for(mode)
            .ok_or_else(|| Error::new("That mode isn't available on this machine."))?;
        match self.caps.mode_backend {
            Some(ModeBackend::MsiEc) => {
                if self.hw.read(&self.ec("shift_mode")).as_deref() != Some(name) {
                    self.hw.write_verified(&self.ec("shift_mode"), name)?;
                }
                self.sync_fan_mode(Some(mode), self.preset())
            }
            Some(ModeBackend::PlatformProfile) => {
                if self.hw.read(PLATFORM_PROFILE).as_deref() != Some(name) {
                    self.hw.write_verified(PLATFORM_PROFILE, name)?;
                }
                Ok(())
            }
            Some(ModeBackend::PowerProfilesDaemon) => {
                self.run.run(POWERPROFILESCTL, &["set", name])
                    .map_err(|e| Error::new(format!("power-profiles-daemon refused {name}: {e}")))?;
                *self.ppd_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
                match self.ppd_current() {
                    Some(now) if now == name => Ok(()),
                    now => Err(Error::new(format!("power-profiles-daemon reports {:?} after setting {name}", now.unwrap_or_default()))),
                }
            }
            None => Err(Error::new("Performance modes aren't available on this machine.")),
        }
    }

    fn apply_cooler_boost(&self, on: bool) -> Result<()> {
        if !self.caps.cooler_boost {
            return Err(Error::new("Max Fans isn't available on this laptop."));
        }
        self.hw.write_verified(&self.ec("cooler_boost"), if on { "on" } else { "off" })
    }

    fn apply_charge_limit(&self, limit: u8) -> Result<()> {
        if !CHARGE_LIMITS.contains(&limit) {
            return Err(Error::new("Charge limit must be 60, 80 or 100%."));
        }
        let dir = self.caps.battery.as_ref()
            .ok_or_else(|| Error::new("This laptop's battery doesn't support a charge limit."))?;
        let end = format!("{dir}/charge_control_end_threshold");
        if !self.caps.battery_linked_start {
            return self.hw.write_verified(&end, &limit.to_string());
        }
        // Drivers with a separate start threshold: keep start 10% below end,
        // ordering the two writes so start < end holds the whole time.
        let start = format!("{dir}/charge_control_start_threshold");
        let new_start = (limit - 10).to_string();
        let lowering = self.hw.read_u64(&end).is_some_and(|cur| (limit as u64) < cur);
        if lowering {
            self.hw.write_verified(&start, &new_start)?;
            self.hw.write_verified(&end, &limit.to_string())
        } else {
            self.hw.write_verified(&end, &limit.to_string())?;
            self.hw.write_verified(&start, &new_start)
        }
    }

    fn apply_turbo(&self, on: bool) -> Result<()> {
        match self.caps.turbo {
            Some(TurboSwitch::IntelNoTurbo) => self.hw.write_verified(caps::INTEL_NO_TURBO, if on { "0" } else { "1" }),
            Some(TurboSwitch::CpufreqBoost) => self.hw.write_verified(caps::CPUFREQ_BOOST, if on { "1" } else { "0" }),
            None => Err(Error::new("Turbo Boost can't be switched on this CPU.")),
        }
    }

    fn write_power_uw(&self, constraint: u8, uw: u64) -> Result<()> {
        let z = self.caps.rapl.as_ref().ok_or_else(|| Error::new("CPU power limits aren't available."))?;
        let path = format!("{z}/constraint_{constraint}_power_limit_uw");
        self.hw.write_raw(&path, &uw.to_string())?;
        // The driver may round to its own step; accept anything within 1 W.
        match self.hw.read_u64(&path) {
            Some(got) if got.abs_diff(uw) <= 1_000_000 => Ok(()),
            got => Err(Error::new(format!("CPU power limit reads back {got:?} µW after writing {uw} µW"))),
        }
    }

    fn apply_power_uw(&self, pl1: u64, pl2: u64) -> Result<()> {
        let z = self.caps.rapl.as_ref().ok_or_else(|| Error::new("CPU power limits aren't available."))?;
        let cur_pl1 = self.hw.read_u64(&format!("{z}/constraint_0_power_limit_uw")).unwrap_or(0);
        // Keep PL1 <= PL2 at every step.
        if pl2 < cur_pl1 {
            self.write_power_uw(0, pl1)?;
            self.write_power_uw(1, pl2)
        } else {
            self.write_power_uw(1, pl2)?;
            self.write_power_uw(0, pl1)
        }
    }

    fn check_power(&self, pl1: u32, pl2: u32) -> Result<()> {
        let l = self.power_limits().ok_or_else(|| Error::new("CPU power limits aren't available."))?;
        if !(l.pl1_min..=l.pl1_max).contains(&pl1) {
            return Err(Error::new(format!("Sustained power must be {}–{} W.", l.pl1_min, l.pl1_max)));
        }
        if !(l.pl2_min..=l.pl2_max).contains(&pl2) {
            return Err(Error::new(format!("Short burst power must be {}–{} W.", l.pl2_min, l.pl2_max)));
        }
        if pl1 > pl2 {
            return Err(Error::new("Sustained power can't be higher than short burst power."));
        }
        Ok(())
    }

    fn fan_target(&self, preset: Preset) -> Result<Tables> {
        let factory = self.factory.fan_tables.as_ref()
            .ok_or_else(|| Error::new("The laptop's default fan curve hasn't been recorded yet."))?;
        Ok(match preset {
            Preset::Custom => self.settings.custom_curve.unwrap_or(*factory),
            p => fan::preset(factory, p).expect("non-custom presets always resolve"),
        })
    }

    fn write_fan_tables(&self, target: &Tables, reduces_cooling: bool) -> Result<()> {
        if !self.caps.fan_curves {
            return Err(Error::new(self.caps.fan_curve_reason.clone()
                .unwrap_or_else(|| "Fan curves aren't available.".into())));
        }
        if matches!(self.fan_test, FanTest::Running { .. }) {
            return Err(Error::new("A fan test is running. Wait for it to finish."));
        }
        let layout = self.fan_layout();
        let before = fan::read(&self.hw, layout)?;
        fan::plausible(&before, layout)?;
        let mut target = *target;
        if !layout.gpu { target.gpu = target.cpu; }
        let target = &target;
        if before == *target {
            return Ok(());
        }
        if reduces_cooling {
            if let Some(t) = fan::hottest(&self.hw) {
                if t >= fan::HOT_C {
                    return Err(Error::new(format!(
                        "The laptop is at {t}°C. Let it cool down before choosing a quieter fan curve."
                    )));
                }
            }
        }
        fan::write(&self.hw, target, &before, layout)
    }

    fn apply_fan_preset(&self, preset: Preset, mode: Option<Mode>) -> Result<()> {
        let target = self.fan_target(preset)?;
        self.write_fan_tables(&target, matches!(preset, Preset::Quiet | Preset::Custom))?;
        self.sync_fan_mode(mode, preset)
    }

    // ---------- public actions: apply, then remember ----------

    pub fn set_mode(&mut self, mode: Mode) -> Result<()> {
        self.apply_mode(mode)?;
        self.settings.mode = Some(mode);
        self.save()?;
        // Some firmware resets RAPL when the shift mode changes.
        if let Some((a, b)) = self.settings.cpu_power {
            self.apply_power_uw(a as u64 * 1_000_000, b as u64 * 1_000_000)?;
        }
        Ok(())
    }

    // ---------- fan curves on MSI models ----------

    fn fan_verified_here(&self) -> bool {
        self.caps.fan_builtin
            || (self.caps.ec_firmware.is_some() && self.factory.fan_verified_fw == self.caps.ec_firmware)
    }

    pub fn fan_layout(&self) -> fan::Layout {
        if self.caps.fan_builtin { fan::FULL } else { fan::Layout { gpu: self.factory.fan_verified_gpu.unwrap_or(true) } }
    }

    /// Load ec_sys so the fan tables can be read (all MSI laptops with msi-ec),
    /// and with write support only where fan curves are verified.
    fn prepare_ec(&mut self) {
        if !self.caps.msi_ec || !self.caps.has_fan_mode("advanced") || !self.hw.exists(caps::MODPROBE) {
            return;
        }
        let loaded = self.hw.exists(caps::EC_IO);
        let writable = self.hw.read(caps::EC_WRITE_SUPPORT).as_deref() == Some("Y");
        if self.fan_verified_here() && !writable {
            self.enable_ec_writes();
        } else if !loaded {
            let _ = self.run.run(caps::MODPROBE, &["ec_sys"]);
        }
        self.caps = caps::detect(&self.hw);
    }

    fn enable_ec_writes(&self) -> bool {
        if self.hw.read(caps::EC_WRITE_SUPPORT).as_deref() == Some("Y") {
            return true;
        }
        if self.hw.exists("/sys/module/ec_sys") {
            let _ = self.run.run(caps::MODPROBE, &["-r", "ec_sys"]);
        }
        let _ = self.run.run(caps::MODPROBE, &["ec_sys", "write_support=1"]);
        self.hw.read(caps::EC_WRITE_SUPPORT).as_deref() == Some("Y")
    }

    /// A fan test was interrupted (service stopped, crash, power loss):
    /// put the laptop's own tables and fan mode back.
    fn recover_fan_test(&mut self) {
        let Some(marker) = self.hw.read(FAN_TEST_FILE).filter(|m| !m.trim().is_empty()) else { return };
        if let Some(t) = self.factory.fan_tables {
            if self.enable_ec_writes() {
                if let Ok(now) = fan::read(&self.hw, fan::FULL) {
                    let _ = fan::write(&self.hw, &t, &now, fan::FULL);
                }
            }
        }
        if let Some(m) = marker.trim().strip_prefix("mode=").filter(|m| !m.is_empty()) {
            let _ = self.hw.write_verified(&self.ec("fan_mode"), m);
        }
        let _ = self.hw.save_private(FAN_TEST_FILE, "");
    }

    fn refresh_fan(&mut self) {
        let verified = self.fan_verified_here();
        let c = &mut self.caps;
        c.fan_curve_reason = if !c.msi_ec {
            Some("Fan curves need the msi-ec driver.".into())
        } else if !c.has_fan_mode("advanced") {
            Some("This laptop's msi-ec driver doesn't offer the advanced fan mode.".into())
        } else if !verified {
            Some("Fan curves haven't been verified on this model yet.".into())
        } else if !self.hw.exists(caps::EC_IO) {
            Some("The ec_sys kernel module is not loaded.".into())
        } else if self.hw.read(caps::EC_WRITE_SUPPORT).as_deref() != Some("Y") {
            Some("The ec_sys kernel module was loaded without write support.".into())
        } else {
            None
        };
        c.fan_curves = c.fan_curve_reason.is_none();
    }

    /// The fan test can unlock fan curves here: an MSI laptop with msi-ec's
    /// advanced fan mode, a readable fan speed, and a CPU table that looks right.
    pub fn fan_test_available(&self) -> bool {
        !self.caps.fan_curves
            && !self.caps.fan_builtin
            && self.caps.msi_ec
            && self.caps.has_fan_mode("advanced")
            && fan::fan_speed(&self.hw, "cpu").is_some()
            && fan::read(&self.hw, fan::FULL).is_ok_and(|t| fan::curve_plausible(&t.cpu))
    }

    /// Start the fan test. Raises the CPU fan curve to 100%, checks the fan
    /// speeds up, then does the same for the GPU curve. Advanced by
    /// `fan_test_tick`; the original tables always go back.
    pub fn start_fan_test(&mut self) -> Result<()> {
        if matches!(self.fan_test, FanTest::Running { .. }) {
            return Err(Error::new("The fan test is already running."));
        }
        if !self.fan_test_available() {
            return Err(Error::new(self.caps.fan_curve_reason.clone().unwrap_or_else(|| "The fan test isn't available on this laptop.".into())));
        }
        if !self.enable_ec_writes() {
            return Err(Error::new("Couldn't turn on write access to the embedded controller (ec_sys)."));
        }
        let before = fan::read(&self.hw, fan::FULL)?;
        let baseline = fan::fan_speed(&self.hw, "cpu").unwrap_or(0);
        if baseline >= 70 {
            return Err(Error::new("The fans are already running fast. Run the test while the laptop is idle."));
        }
        // These are the laptop's own defaults: keep them for Reset and the presets.
        if self.factory.fan_tables.is_none() {
            self.factory.fan_tables = Some(before);
            let _ = self.hw.save_private(FACTORY_FILE, &self.factory.serialize());
        }
        let mode_before = self.hw.read(&self.ec("fan_mode"));
        // Marker so a crash or power loss mid-test still puts the tables back.
        self.hw.save_private(FAN_TEST_FILE, &format!("mode={}\n", mode_before.clone().unwrap_or_default()))?;
        let mut test = before;
        test.cpu.speeds = [100; fan::POINTS];
        fan::write(&self.hw, &test, &before, fan::CPU_ONLY)?;
        if let Err(e) = self.hw.write_verified(&self.ec("fan_mode"), "advanced") {
            let _ = fan::write(&self.hw, &before, &test, fan::FULL);
            return Err(e);
        }
        self.fan_test = FanTest::Running {
            phase: TestPhase::Cpu,
            since: Instant::now(),
            before,
            mode_before,
            baseline,
            gpu_candidate: fan::curve_plausible(&before.gpu),
        };
        Ok(())
    }

    pub fn cancel_fan_test(&mut self) {
        if let FanTest::Running { before, mode_before, .. } = self.fan_test.clone() {
            self.finish_fan_test(&before, mode_before.as_deref(), false, None, "The fan test was cancelled.".into());
        }
    }

    fn finish_fan_test(&mut self, before: &Tables, mode_before: Option<&str>, ok: bool, gpu: Option<bool>, message: String) {
        if let Ok(now) = fan::read(&self.hw, fan::FULL) {
            let _ = fan::write(&self.hw, before, &now, fan::FULL);
        }
        if let Some(m) = mode_before {
            let _ = self.hw.write_verified(&self.ec("fan_mode"), m);
        }
        let _ = self.hw.save_private(FAN_TEST_FILE, "");
        if ok {
            self.factory.fan_verified_fw = self.caps.ec_firmware.clone();
            self.factory.fan_verified_gpu = gpu;
            let _ = self.hw.save_private(FACTORY_FILE, &self.factory.serialize());
            self.refresh_fan();
            self.capture_factory();
        }
        self.fan_test = FanTest::Done { ok, message };
    }

    /// Call every couple of seconds; returns log lines.
    pub fn fan_test_tick(&mut self) -> Vec<String> {
        let FanTest::Running { phase, since, before, mode_before, baseline, gpu_candidate } = self.fan_test.clone() else {
            return Vec::new();
        };
        let (wait, settle) = self.fan_test_timing;
        let responded = |speed: Option<u64>, base: u64| speed.is_some_and(|s| s >= base + FAN_TEST_RISE || s >= FAN_TEST_FAST);
        match phase {
            TestPhase::Cpu => {
                if responded(fan::fan_speed(&self.hw, "cpu"), baseline) {
                    if gpu_candidate {
                        // CPU table confirmed: put it back and let the fans settle before the GPU.
                        if let Ok(now) = fan::read(&self.hw, fan::FULL) {
                            let _ = fan::write(&self.hw, &before, &now, fan::FULL);
                        }
                        self.fan_test = FanTest::Running { phase: TestPhase::Settle, since: Instant::now(), before, mode_before, baseline, gpu_candidate };
                        return vec!["Fan test: CPU fan curve confirmed.".into()];
                    }
                    self.finish_fan_test(&before, mode_before.as_deref(), true, Some(false),
                        "Fan curves verified. This laptop has one fan curve for all fans.".into());
                    return vec!["Fan test passed (single curve).".into()];
                }
                if since.elapsed() > wait {
                    self.finish_fan_test(&before, mode_before.as_deref(), false, None,
                        "The fans didn't respond to the test curve, so fan curves stay off on this model. Copy the system report and send it in so it can be looked at.".into());
                    return vec!["Fan test failed: CPU fan did not respond.".into()];
                }
            }
            TestPhase::Settle => {
                if since.elapsed() > settle {
                    let gpu_base = fan::fan_speed(&self.hw, "gpu").unwrap_or(0);
                    let mut test = before;
                    test.gpu.speeds = [100; fan::POINTS];
                    if gpu_base >= 70 || fan::write(&self.hw, &test, &before, fan::FULL).is_err() {
                        self.finish_fan_test(&before, mode_before.as_deref(), true, Some(false),
                            "Fan curves verified for the CPU curve; the GPU curve couldn't be tested, so one curve is used for all fans.".into());
                        return vec!["Fan test passed (CPU curve only).".into()];
                    }
                    self.fan_test = FanTest::Running { phase: TestPhase::Gpu, since: Instant::now(), before, mode_before, baseline: gpu_base, gpu_candidate };
                }
            }
            TestPhase::Gpu => {
                if responded(fan::fan_speed(&self.hw, "gpu"), baseline) {
                    self.finish_fan_test(&before, mode_before.as_deref(), true, Some(true),
                        "Fan curves verified for both the CPU and GPU fans.".into());
                    return vec!["Fan test passed (CPU and GPU curves).".into()];
                }
                if since.elapsed() > wait {
                    self.finish_fan_test(&before, mode_before.as_deref(), true, Some(false),
                        "Fan curves verified. The GPU table didn't respond, so this laptop uses one curve for all fans.".into());
                    return vec!["Fan test passed (single curve; GPU table did not respond).".into()];
                }
            }
        }
        Vec::new()
    }

    fn fan_test_json(&self) -> J {
        let (running, phase, ok, message) = match &self.fan_test {
            FanTest::Idle => (false, None, None, None),
            FanTest::Running { phase, .. } => (true, Some(match phase {
                TestPhase::Cpu => "cpu", TestPhase::Settle => "settle", TestPhase::Gpu => "gpu",
            }), None, None),
            FanTest::Done { ok, message } => (false, None, Some(*ok), Some(message.clone())),
        };
        J::Obj(vec![
            ("available", J::Bool(running || self.fan_test_available())),
            ("running", J::Bool(running)),
            ("phase", J::opt(phase)),
            ("ok", J::opt(ok)),
            ("message", J::opt(message)),
        ])
    }

    #[cfg(test)]
    pub(crate) fn set_fan_test_timing(&mut self, wait: Duration, settle: Duration) {
        self.fan_test_timing = (wait, settle);
    }

    // ---------- MSI extras ----------

    fn read_onoff(&self, name: &str) -> Option<bool> {
        self.hw.read(&self.ec(name)).map(|v| v == "on")
    }

    fn write_onoff(&self, name: &str, on: bool) -> Result<()> {
        self.hw.write_verified(&self.ec(name), if on { "on" } else { "off" })
    }

    pub fn set_webcam(&mut self, on: bool) -> Result<()> {
        if !self.caps.webcam {
            return Err(Error::new("This laptop's webcam can't be switched by FrostRig."));
        }
        if on && self.read_onoff("webcam_block") == Some(true) {
            return Err(Error::new("The webcam is blocked. Turn off Block webcam first."));
        }
        self.write_onoff("webcam", on)?;
        self.settings.webcam = Some(on);
        self.save()
    }

    /// Blocking also switches the webcam off, so it's off for as long as it's blocked.
    pub fn set_webcam_block(&mut self, on: bool) -> Result<()> {
        if !self.caps.webcam_block {
            return Err(Error::new("This laptop can't block its webcam."));
        }
        if on && self.caps.webcam && self.read_onoff("webcam") == Some(true) {
            self.write_onoff("webcam", false)?;
            self.settings.webcam = Some(false);
        }
        self.write_onoff("webcam_block", on)?;
        self.settings.webcam_block = Some(on);
        self.save()
    }

    /// "left" or "right": which side of the Windows key the Fn key sits on.
    pub fn set_fn_key(&mut self, side: &str) -> Result<()> {
        if !self.caps.fn_key {
            return Err(Error::new("This laptop can't swap its Fn and Windows keys."));
        }
        if side != "left" && side != "right" {
            return Err(Error::new("The Fn key can be on the left or the right."));
        }
        self.hw.write_verified(&self.ec("fn_key"), side)?;
        self.settings.fn_key = Some(side.to_owned());
        self.save()
    }

    pub fn set_super_battery(&mut self, on: bool) -> Result<()> {
        if !self.caps.super_battery {
            return Err(Error::new("Super Battery isn't available on this laptop."));
        }
        self.write_onoff("super_battery", on)?;
        self.settings.super_battery = Some(on);
        self.save()
    }

    pub fn set_kbd_backlight(&mut self, level: u64) -> Result<()> {
        let (dir, max) = self.caps.kbd_backlight.clone()
            .ok_or_else(|| Error::new("This keyboard's backlight can't be set by FrostRig."))?;
        if level > max {
            return Err(Error::new(format!("Keyboard backlight goes from 0 to {max}.")));
        }
        self.hw.write_verified(&format!("{dir}/brightness"), &level.to_string())?;
        self.settings.kbd_backlight = Some(level);
        self.save()
    }

    fn restore_extras(&self, run: &mut dyn FnMut(&str, Result<()>)) {
        let s = &self.settings;
        // Block last when blocking (camera off first), first when unblocking.
        match s.webcam_block {
            Some(true) if self.caps.webcam_block => {
                if self.caps.webcam { run("Webcam", self.write_onoff("webcam", false)); }
                run("Webcam block", self.write_onoff("webcam_block", true));
            }
            other => {
                if let (Some(false), true) = (other, self.caps.webcam_block) {
                    run("Webcam block", self.write_onoff("webcam_block", false));
                }
                if let (Some(on), true) = (s.webcam, self.caps.webcam) {
                    run("Webcam", self.write_onoff("webcam", on));
                }
            }
        }
        if let (Some(side), true) = (&s.fn_key, self.caps.fn_key) {
            run("Fn key", self.hw.write_verified(&self.ec("fn_key"), side));
        }
        if let (Some(on), true) = (s.super_battery, self.caps.super_battery) {
            run("Super Battery", self.write_onoff("super_battery", on));
        }
        if let (Some(level), Some((dir, max))) = (s.kbd_backlight, &self.caps.kbd_backlight) {
            if level <= *max {
                run("Keyboard backlight", self.hw.write_verified(&format!("{dir}/brightness"), &level.to_string()));
            }
        }
    }

    fn extras_json(&self) -> J {
        let c = &self.caps;
        let onoff = |ok: bool, name: &str| if ok { J::opt(self.read_onoff(name)) } else { J::Null };
        J::Obj(vec![
            ("webcam", onoff(c.webcam, "webcam")),
            ("webcamBlock", onoff(c.webcam_block, "webcam_block")),
            ("fnKey", if c.fn_key { J::opt(self.hw.read(&self.ec("fn_key"))) } else { J::Null }),
            ("superBattery", onoff(c.super_battery, "super_battery")),
            ("kbdBacklight", match &c.kbd_backlight {
                Some((dir, max)) => J::Obj(vec![
                    ("level", J::opt(self.hw.read_u64(&format!("{dir}/brightness")))),
                    ("max", (*max).into()),
                ]),
                None => J::Null,
            }),
        ])
    }

    pub fn set_cooler_boost(&mut self, on: bool) -> Result<()> {
        self.apply_cooler_boost(on)?;
        self.settings.cooler_boost = Some(on);
        self.save()
    }

    pub fn set_charge_limit(&mut self, limit: u8) -> Result<()> {
        self.apply_charge_limit(limit)?;
        self.settings.charge_limit = Some(limit);
        self.save()
    }

    pub fn set_turbo(&mut self, on: bool) -> Result<()> {
        self.apply_turbo(on)?;
        self.settings.turbo = Some(on);
        self.save()
    }

    pub fn set_cpu_power(&mut self, pl1: u32, pl2: u32) -> Result<()> {
        self.check_power(pl1, pl2)?;
        self.apply_power_uw(pl1 as u64 * 1_000_000, pl2 as u64 * 1_000_000)?;
        self.settings.cpu_power = Some((pl1, pl2));
        self.save()
    }

    pub fn set_fan_preset(&mut self, preset: Preset) -> Result<()> {
        let mode = self.live_mode().or(self.settings.mode);
        self.apply_fan_preset(preset, mode)?;
        self.settings.fan_preset = Some(preset);
        self.save()
    }

    pub fn set_custom_curve(&mut self, mut curve: Tables) -> Result<()> {
        // Single-curve boards: the CPU curve drives every fan.
        if !self.fan_layout().gpu { curve.gpu = curve.cpu; }
        fan::validate_custom(&curve)?;
        self.write_fan_tables(&curve, true)?;
        let mode = self.live_mode().or(self.settings.mode);
        self.sync_fan_mode(mode, Preset::Custom)?;
        self.settings.fan_preset = Some(Preset::Custom);
        self.settings.custom_curve = Some(curve);
        self.save()
    }

    /// Fan curve, CPU power, Turbo and GPU settings back to the laptop's defaults.
    /// A saved custom curve is kept so the user can go back to it.
    pub fn reset_advanced(&mut self) -> Result<()> {
        let mut errors = Vec::new();
        if self.caps.fan_curves && self.factory.fan_tables.is_some() {
            match self.set_fan_preset(Preset::Auto) {
                Ok(()) => self.settings.fan_preset = None,
                Err(e) => errors.push(e.0),
            }
        }
        if let (Some(a), Some(b)) = (self.factory.pl1_uw, self.factory.pl2_uw) {
            if self.caps.rapl.is_some() {
                match self.apply_power_uw(a, b) {
                    Ok(()) => self.settings.cpu_power = None,
                    Err(e) => errors.push(e.0),
                }
            }
        }
        if let Some(t) = self.factory.turbo {
            if self.caps.turbo.is_some() {
                match self.apply_turbo(t) {
                    Ok(()) => self.settings.turbo = None,
                    Err(e) => errors.push(e.0),
                }
            }
        }
        for g in self.gpus.clone() {
            let Some(set) = self.settings.gpus.get(&g.pci).cloned() else { continue };
            if gpu::awake(&self.hw, &g) {
                if set.clock_mhz.is_some() {
                    if let Err(e) = gpu::set_clock_cap(&self.hw, self.run.as_ref(), &g, None) { errors.push(e.0); }
                }
                if set.power_w.is_some() {
                    if let Some(r) = self.gpu_power_range(&g) {
                        if let Err(e) = gpu::set_power(&self.hw, self.run.as_ref(), &g, r.default_w) { errors.push(e.0); }
                    }
                }
                if set.level.is_some() {
                    if let Err(e) = gpu::set_amd_level(&self.hw, &g, "auto") { errors.push(e.0); }
                }
            }
            // Asleep: the driver resets everything itself when the GPU powers on.
            self.settings.gpus.remove(&g.pci);
        }
        if self.settings.dynamic_boost.is_some() && self.boost_available() {
            // Back to whatever the system does on its own at boot.
            match gpu::set_boost(self.run.as_ref(), gpu::boost_enabled_by_system(self.run.as_ref())) {
                Ok(()) => self.settings.dynamic_boost = None,
                Err(e) => errors.push(e.0),
            }
        }
        self.invalidate_gpu();
        self.save()?;
        if errors.is_empty() { Ok(()) } else { Err(Error::new(errors.join(" "))) }
    }

    // ---------- GPU ----------

    fn invalidate_gpu(&self) {
        if let Ok(mut c) = self.gpu_live.lock() { c.clear(); }
        if let Ok(mut c) = self.boost_cache.lock() { *c = None; }
    }

    /// v0.16 stored one NVIDIA clock limit and top clock without a PCI address.
    fn migrate_legacy_gpu(&mut self) {
        let Some(nv) = self.gpus.iter().find(|g| g.backend() == Some(Vendor::Nvidia)).map(|g| g.pci.clone()) else { return };
        if let Some(v) = self.settings.legacy_gpu_clock.take() {
            self.settings.gpus.entry(nv.clone()).or_default().clock_mhz = Some(v);
            let _ = self.save();
        }
        if let Some(v) = self.factory.legacy_gpu_max_clock.take() {
            self.factory.gpu_max_clock.entry(nv).or_insert(v);
            let _ = self.hw.save_private(FACTORY_FILE, &self.factory.serialize());
        }
    }

    pub fn gpus(&self) -> &[Gpu] {
        &self.gpus
    }

    /// GPUs worth showing controls for: the dedicated ones if there are any
    /// (a gaming laptop's built-in Intel GPU isn't interesting), otherwise all.
    pub fn shown_gpus(&self) -> Vec<&Gpu> {
        let dedicated: Vec<&Gpu> = self.gpus.iter().filter(|g| !g.integrated).collect();
        if dedicated.is_empty() { self.gpus.iter().collect() } else { dedicated }
    }

    fn find_gpu(&self, pci: &str) -> Result<Gpu> {
        self.gpus.iter().find(|g| g.pci == pci).cloned()
            .ok_or_else(|| Error::new(format!("No GPU at {pci}.")))
    }

    pub fn boost_available(&self) -> bool {
        self.hw.exists(gpu::POWERD_UNIT_FILE) && self.gpus.iter().any(|g| g.backend() == Some(Vendor::Nvidia))
    }

    pub fn gpu_clock_range(&self, g: &Gpu) -> Option<gpu::ClockRange> {
        match g.backend()? {
            Vendor::Nvidia => {
                let max = *self.factory.gpu_max_clock.get(&g.pci)?;
                Some(gpu::ClockRange { min: gpu::NVIDIA_MIN_CLOCK_MHZ.min(max), max, step: gpu::NVIDIA_CLOCK_STEP_MHZ })
            }
            Vendor::Intel => self.gpu_probe.get(&g.pci)?.intel_clock,
            Vendor::Amd => None,
        }
    }

    fn gpu_power_range(&self, g: &Gpu) -> Option<gpu::PowerRange> {
        self.gpu_probe.get(&g.pci)?.power
    }

    /// Cap a GPU's clock, or remove the cap with None. Saved and applied when
    /// the GPU wakes if it's asleep now.
    pub fn set_gpu_clock_limit(&mut self, pci: &str, mhz: Option<u32>) -> Result<()> {
        let g = self.find_gpu(pci)?;
        let mhz = match mhz {
            None => None,
            Some(m) => {
                let r = self.gpu_clock_range(&g).ok_or_else(|| Error::new(
                    "This GPU's clock range isn't known yet. Run something that uses the GPU, then try again."))?;
                if !(r.min..=r.max).contains(&m) {
                    return Err(Error::new(format!("GPU clock limit must be {}–{} MHz.", r.min, r.max)));
                }
                // The top of the range means "no limit".
                if m == r.max { None } else { Some(m) }
            }
        };
        if gpu::awake(&self.hw, &g) {
            gpu::set_clock_cap(&self.hw, self.run.as_ref(), &g, mhz)?;
        }
        self.settings.gpus.entry(g.pci.clone()).or_default().clock_mhz = mhz;
        self.tidy_gpu_settings();
        self.invalidate_gpu();
        self.save()
    }

    /// Set a GPU's power limit in watts, or back to its default with None.
    pub fn set_gpu_power_limit(&mut self, pci: &str, watts: Option<u32>) -> Result<()> {
        let g = self.find_gpu(pci)?;
        let r = self.gpu_power_range(&g).ok_or_else(|| Error::new("This GPU's power limit can't be changed."))?;
        if let Some(w) = watts {
            if !(r.min_w..=r.max_w).contains(&w) {
                return Err(Error::new(format!("GPU power limit must be {}–{} W.", r.min_w, r.max_w)));
            }
        }
        let watts = watts.filter(|w| *w != r.default_w);
        if gpu::awake(&self.hw, &g) {
            gpu::set_power(&self.hw, self.run.as_ref(), &g, watts.unwrap_or(r.default_w))?;
        }
        self.settings.gpus.entry(g.pci.clone()).or_default().power_w = watts;
        self.tidy_gpu_settings();
        self.invalidate_gpu();
        self.save()
    }

    /// AMD only: "auto", "low" (power saving) or "high" (maximum clocks).
    pub fn set_gpu_level(&mut self, pci: &str, level: &str) -> Result<()> {
        let g = self.find_gpu(pci)?;
        if g.backend() != Some(Vendor::Amd) {
            return Err(Error::new("Performance levels are only available on AMD GPUs."));
        }
        if !gpu::AMD_LEVELS.contains(&level) {
            return Err(Error::new(format!("Unknown GPU performance level {level:?}.")));
        }
        if gpu::awake(&self.hw, &g) {
            gpu::set_amd_level(&self.hw, &g, level)?;
        }
        self.settings.gpus.entry(g.pci.clone()).or_default().level = (level != "auto").then(|| level.to_owned());
        self.tidy_gpu_settings();
        self.invalidate_gpu();
        self.save()
    }

    fn tidy_gpu_settings(&mut self) {
        self.settings.gpus.retain(|_, g| !g.is_empty());
    }

    pub fn set_dynamic_boost(&mut self, on: bool) -> Result<()> {
        if !self.boost_available() {
            return Err(Error::new("Dynamic Boost (nvidia-powerd) isn't installed."));
        }
        gpu::set_boost(self.run.as_ref(), on)?;
        self.settings.dynamic_boost = Some(on);
        self.invalidate_gpu();
        self.save()
    }

    /// Learn what a freshly woken GPU can do. The power test write uses the
    /// value the driver already has, so nothing changes.
    fn probe_gpu(&mut self, g: &Gpu) {
        let mut probe = self.gpu_probe.remove(&g.pci).unwrap_or_default();
        let live = gpu::read_live(&self.hw, self.run.as_ref(), g);
        if live.name.is_some() { probe.name = live.name.clone(); }
        if g.backend() == Some(Vendor::Nvidia) && !self.factory.gpu_max_clock.contains_key(&g.pci) {
            if let Some(max) = live.max_clock_mhz {
                self.factory.gpu_max_clock.insert(g.pci.clone(), max);
                let _ = self.hw.save_private(FACTORY_FILE, &self.factory.serialize());
            }
        }
        if g.backend() == Some(Vendor::Intel) {
            probe.intel_clock = gpu::intel_clock_range(&self.hw, g);
        }
        if !probe.power_checked {
            probe.power_checked = true;
            probe.power = gpu::power_info(&self.hw, self.run.as_ref(), g).and_then(|(range, current)| {
                gpu::set_power(&self.hw, self.run.as_ref(), g, current).ok().map(|_| range)
            });
        }
        self.gpu_probe.insert(g.pci.clone(), probe);
    }

    fn apply_gpu_settings(&self, g: &Gpu, s: &GpuSettings) -> Vec<String> {
        let mut log = Vec::new();
        if let Some(m) = s.clock_mhz {
            match gpu::set_clock_cap(&self.hw, self.run.as_ref(), g, Some(m)) {
                Ok(()) => log.push(format!("GPU {}: clock limit {m} MHz applied.", g.pci)),
                Err(e) => log.push(format!("GPU {}: could not apply the clock limit: {e}", g.pci)),
            }
        }
        if let Some(w) = s.power_w {
            if self.gpu_power_range(g).is_some() {
                match gpu::set_power(&self.hw, self.run.as_ref(), g, w) {
                    Ok(()) => log.push(format!("GPU {}: power limit {w} W applied.", g.pci)),
                    Err(e) => log.push(format!("GPU {}: could not apply the power limit: {e}", g.pci)),
                }
            }
        }
        if let Some(l) = &s.level {
            match gpu::set_amd_level(&self.hw, g, l) {
                Ok(()) => log.push(format!("GPU {}: performance level {l} applied.", g.pci)),
                Err(e) => log.push(format!("GPU {}: could not apply the performance level: {e}", g.pci)),
            }
        }
        log
    }

    /// Call every few seconds. When a GPU has just woken up (or after
    /// `redetect`), learn what it supports and put its settings back, since
    /// drivers forget them whenever the GPU powers off. Reading the wake state
    /// never wakes a GPU.
    pub fn gpu_watch(&mut self) -> Vec<String> {
        let mut log = Vec::new();
        for g in self.gpus.clone() {
            if g.backend().is_none() { continue; }
            let awake = gpu::awake(&self.hw, &g);
            let was = self.gpu_was_awake.get(&g.pci).copied().unwrap_or(false);
            if awake && !was {
                self.probe_gpu(&g);
                if let Some(s) = self.settings.gpus.get(&g.pci).cloned() {
                    log.extend(self.apply_gpu_settings(&g, &s));
                }
                self.invalidate_gpu();
            }
            self.gpu_was_awake.insert(g.pci.clone(), awake);
        }
        log
    }

    /// Read the CPU energy counter; called every couple of seconds by the
    /// service so `cpuPowerW` is always the draw over the last interval.
    pub fn sample_cpu_power(&mut self) {
        self.sample_cpu_power_at(Instant::now());
    }

    pub(crate) fn sample_cpu_power_at(&mut self, at: Instant) {
        let zone = cpu::energy_zone(&self.hw);
        let Some(uj) = zone.as_ref().and_then(|z| self.hw.read_u64(&format!("{z}/energy_uj"))) else {
            self.cpu_energy = None;
            return;
        };
        let watts = self.cpu_energy.and_then(|prev| {
            let range = zone.as_ref().and_then(|z| self.hw.read_u64(&format!("{z}/max_energy_range_uj")));
            cpu::watts(prev.uj, uj, range, at.checked_duration_since(prev.at)?.as_secs_f64())
        });
        self.cpu_energy = Some(EnergySample { at, uj, watts });
    }

    fn cpu_power_w(&self) -> Option<f64> {
        let s = self.cpu_energy?;
        if s.at.elapsed() > CPU_POWER_FRESH { return None; }
        s.watts
    }

    fn gpu_live(&self, g: &Gpu) -> Option<gpu::Live> {
        // Never read a sleeping GPU: it would wake it.
        if !gpu::awake(&self.hw, g) { return None; }
        let mut cache = self.gpu_live.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, live)) = cache.get(&g.pci) {
            if at.elapsed() < GPU_CACHE { return Some(live.clone()); }
        }
        let live = gpu::read_live(&self.hw, self.run.as_ref(), g);
        cache.insert(g.pci.clone(), (Instant::now(), live.clone()));
        Some(live)
    }

    fn boost_on(&self) -> bool {
        let mut cache = self.boost_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, on)) = *cache {
            if at.elapsed() < GPU_CACHE { return on; }
        }
        let on = gpu::boost_running(self.run.as_ref());
        *cache = Some((Instant::now(), on));
        on
    }

    fn gpu_json(&self, g: &Gpu) -> J {
        let awake = gpu::awake(&self.hw, g);
        let live = self.gpu_live(g).unwrap_or_default();
        let probe = self.gpu_probe.get(&g.pci);
        let set = self.settings.gpus.get(&g.pci).cloned().unwrap_or_default();
        let name = live.name.clone().or_else(|| probe.and_then(|p| p.name.clone())).unwrap_or_else(|| gpu::default_name(g));
        let clock = self.gpu_clock_range(g).map(|r| J::Obj(vec![
            ("minMhz", r.min.into()), ("maxMhz", r.max.into()), ("stepMhz", r.step.into()),
            ("limitMhz", J::opt(set.clock_mhz)),
        ])).unwrap_or(J::Null);
        let power = self.gpu_power_range(g).map(|r| J::Obj(vec![
            ("minW", r.min_w.into()), ("maxW", r.max_w.into()), ("defaultW", r.default_w.into()),
            ("limitW", J::opt(set.power_w)),
        ])).unwrap_or(J::Null);
        let level = if g.backend() == Some(Vendor::Amd) && self.hw.exists(&gpu::amd_level_file(g)) {
            let current = if awake { gpu::amd_level(&self.hw, g) } else { set.level.clone() };
            J::Obj(vec![
                ("options", gpu::AMD_LEVELS.to_vec().into()),
                ("current", J::str(current.filter(|l| gpu::AMD_LEVELS.contains(&l.as_str())).unwrap_or_else(|| "auto".into()))),
            ])
        } else {
            J::Null
        };
        J::Obj(vec![
            ("id", J::str(&g.pci)),
            ("vendor", J::str(g.vendor.id())),
            ("driver", J::str(&g.driver)),
            ("name", J::Str(name)),
            ("integrated", J::Bool(g.integrated)),
            ("awake", J::Bool(awake)),
            ("tempC", J::opt(live.temp_c)),
            ("powerW", J::opt(live.power_w)),
            ("powerLimitW", J::opt(live.power_limit_w)),
            ("clockMhz", J::opt(live.clock_mhz)),
            ("clock", clock),
            ("power", power),
            ("level", level),
        ])
    }

    /// Put every saved setting back (boot, wake from sleep). Each step is
    /// independent; returns a message per step that failed.
    pub fn restore(&self) -> Vec<String> {
        let s = &self.settings;
        let mut errors = Vec::new();
        let mut run = |what: &str, r: Result<()>| {
            if let Err(e) = r { errors.push(format!("{what}: {e}")); }
        };
        if let Some(v) = s.charge_limit { run("Charge limit", self.apply_charge_limit(v)); }
        if let Some(v) = s.turbo { run("Turbo Boost", self.apply_turbo(v)); }
        if let Some(m) = s.mode { run("Mode", self.apply_mode(m)); }
        if let Some(p) = s.fan_preset {
            if p != Preset::Auto || self.factory.fan_tables.is_some() {
                run("Fan curve", self.apply_fan_preset(p, s.mode.or(self.live_mode())));
            }
        }
        if let Some(v) = s.cooler_boost { run("Max Fans", self.apply_cooler_boost(v)); }
        self.restore_extras(&mut run);
        if let Some((a, b)) = s.cpu_power {
            run("CPU power", self.check_power(a, b)
                .and_then(|_| self.apply_power_uw(a as u64 * 1_000_000, b as u64 * 1_000_000)));
        }
        // The GPU clock limit is put back by gpu_watch, when the GPU is awake.
        if let Some(on) = s.dynamic_boost {
            if self.boost_available() && gpu::boost_running(self.run.as_ref()) != on {
                run("Dynamic Boost", gpu::set_boost(self.run.as_ref(), on));
            }
        }
        errors
    }

    // ---------- status for the app ----------

    fn battery_json(&self) -> J {
        let dir = self.caps.battery.clone().or_else(|| {
            self.hw.list_dir(POWER_SUPPLY).into_iter()
                .map(|n| format!("{POWER_SUPPLY}/{n}"))
                .find(|d| caps::is_system_battery(&self.hw, d))
        });
        let ac = self.hw.list_dir(POWER_SUPPLY).into_iter()
            .map(|n| format!("{POWER_SUPPLY}/{n}"))
            .find(|d| self.hw.read(&format!("{d}/type")).as_deref() == Some("Mains"))
            .and_then(|d| self.hw.read_u64(&format!("{d}/online")))
            .map(|v| v != 0);
        J::Obj(vec![
            ("present", J::Bool(dir.is_some())),
            ("percent", J::opt(dir.as_ref().and_then(|d| self.hw.read_u64(&format!("{d}/capacity"))))),
            ("status", J::opt(dir.as_ref().and_then(|d| self.hw.read(&format!("{d}/status"))))),
            ("acOnline", J::opt(ac)),
            ("chargeLimit", J::opt(self.live_charge_limit())),
        ])
    }

    /// Every fan speed sensor the kernel exposes, on any laptop: each hwmon
    /// device's `fanN_input`, named by `fanN_label` when the driver gives one.
    pub fn fans(&self) -> Vec<(String, u64)> {
        let mut fans = Vec::new();
        let mut devices = self.hw.list_dir("/sys/class/hwmon");
        // hwmon10 after hwmon9, not after hwmon1.
        devices.sort_by_key(|d| d.trim_start_matches("hwmon").parse::<u32>().unwrap_or(u32::MAX));
        for dev in devices {
            let dir = format!("/sys/class/hwmon/{dev}");
            let driver = self.hw.read(&format!("{dir}/name")).unwrap_or_default();
            let mut channels: Vec<u32> = self.hw.list_dir(&dir).iter()
                .filter_map(|f| f.strip_prefix("fan")?.strip_suffix("_input")?.parse().ok())
                .collect();
            channels.sort_unstable();
            for n in channels {
                let Some(rpm) = self.hw.read_u64(&format!("{dir}/fan{n}_input")) else { continue };
                let label = self.hw.read(&format!("{dir}/fan{n}_label")).unwrap_or_else(|| match driver.as_str() {
                    "amdgpu" | "nouveau" => "GPU fan".to_owned(),
                    _ => format!("Fan {}", fans.len() + 1),
                });
                fans.push((label, rpm));
            }
        }
        fans
    }

    fn curve_json(c: &fan::Curve) -> J {
        J::Obj(vec![("temps", c.temps.to_vec().into()), ("speeds", c.speeds.to_vec().into())])
    }

    /// Plain-text summary of what FrostRig found, for bug reports from other
    /// hardware. No serial numbers or personal data.
    pub fn report(&self) -> String {
        let c = &self.caps;
        let rd = |p: &str| self.hw.read(p).unwrap_or_else(|| "-".into());
        let dmi = |f: &str| rd(&format!("{}/{f}", caps::DMI));
        let mut r = String::new();
        let mut line = |k: &str, v: String| r.push_str(&format!("{k:<22}{v}\n"));
        line("FrostRig", VERSION.to_owned());
        line("Kernel", rd("/proc/sys/kernel/osrelease"));
        line("Vendor / model", format!("{} / {}", dmi("sys_vendor"), dmi("product_name")));
        line("Board", format!("{} {}", dmi("board_vendor"), dmi("board_name")));
        line("BIOS", format!("{} ({})", dmi("bios_version"), dmi("bios_date")));
        line("Form factor", caps::form_factor(&self.hw).to_owned());
        line("msi-ec", if c.msi_ec { format!("yes, EC firmware {}", c.ec_firmware.clone().unwrap_or_default()) } else { "no".into() });
        line("Mode backend", c.mode_backend.map(|b| b.id().to_owned()).unwrap_or_else(|| "none".into()));
        line("Modes", c.modes.iter().map(|(m, n)| format!("{}={n}", m.id())).collect::<Vec<_>>().join(", "));
        line("platform_profile", format!("{} [{}]", rd(PLATFORM_PROFILE), rd(caps::PLATFORM_PROFILE_CHOICES)));
        line("Fan modes", c.fan_modes.join(", "));
        line("Max Fans", c.cooler_boost.to_string());
        line("Fan curves", c.fan_curve_reason.clone().unwrap_or_else(|| "available".into()));
        line("Battery", c.battery.clone().unwrap_or_else(|| "none with a charge limit".into()));
        line("Turbo switch", format!("{:?}", c.turbo));
        line("CPU driver", rd(caps::SCALING_DRIVER));
        line("RAPL", c.rapl.clone().unwrap_or_else(|| "none".into()));
        line("Dynamic Boost", if self.boost_available() { "nvidia-powerd installed".into() } else { "not available".into() });
        for g in &self.gpus {
            let probe = self.gpu_probe.get(&g.pci);
            line(&format!("GPU {}", g.pci), format!(
                "{} driver={} integrated={} awake={} backend={} power={} clock={:?}",
                g.vendor.id(), g.driver, g.integrated, gpu::awake(&self.hw, g),
                g.backend().map_or("none", Vendor::id),
                match probe { None => "not probed yet".into(), Some(p) => format!("{:?}", p.power) },
                self.gpu_clock_range(g),
            ));
        }
        let hwmon: Vec<String> = self.hw.list_dir("/sys/class/hwmon").into_iter()
            .filter_map(|d| self.hw.read(&format!("/sys/class/hwmon/{d}/name")).map(|n| format!("{d}={n}")))
            .collect();
        line("hwmon", hwmon.join(", "));
        line("Fan layout", if self.caps.fan_builtin { "built-in (verified model)".into() } else {
            format!("verified fw={:?} gpu={:?}", self.factory.fan_verified_fw, self.factory.fan_verified_gpu)
        });
        if let Some(d) = fan::dump(&self.hw, 0x60, 0x40) {
            r.push_str("\nEC fan area (0x60-0x9F):\n");
            r.push_str(&d);
            r.push('\n');
        }
        r.push_str("\nStatus:\n");
        r.push_str(&self.status_json());
        r.push('\n');
        r
    }

    pub fn status_json(&self) -> String {
        let c = &self.caps;
        let sensor = |part: &str, f: &str| J::opt(self.hw.read_u64(&format!("{MSI_EC}/{part}/{f}")));
        let fan_tables = if c.fan_curves { fan::read(&self.hw, self.fan_layout()).ok() } else { None };
        let power = self.live_power_w();
        let limits = self.power_limits();
        J::Obj(vec![
            ("version", J::str(VERSION)),
            ("device", J::Obj(vec![
                ("model", J::opt(c.model.clone())),
                ("vendor", J::opt(self.hw.read(&format!("{}/sys_vendor", caps::DMI)))),
                ("formFactor", J::str(caps::form_factor(&self.hw))),
                ("board", J::opt(c.board.clone())),
                ("ecFirmware", J::opt(c.ec_firmware.clone())),
                ("msiEc", J::Bool(c.msi_ec)),
            ])),
            ("supported", J::Obj(vec![
                ("modes", c.modes.iter().map(|(m, _)| J::str(m.id())).collect::<Vec<_>>().into()),
                ("modeBackend", J::opt(c.mode_backend.map(ModeBackend::id))),
                ("maxFans", J::Bool(c.cooler_boost)),
                ("chargeLimit", J::Bool(c.battery.is_some())),
                ("turbo", J::Bool(c.turbo.is_some())),
                ("cpuPower", J::Bool(c.rapl.is_some() && limits.is_some())),
                ("fanCurves", J::Bool(c.fan_curves && self.factory.fan_tables.is_some())),
                ("fanCurveReason", J::opt(c.fan_curve_reason.clone())),
                ("fanTest", self.fan_test_json()),
            ])),
            ("state", J::Obj(vec![
                ("mode", J::opt(self.live_mode().map(Mode::id))),
                ("maxFans", J::opt(self.hw.read(&self.ec("cooler_boost")).map(|v| v == "on"))),
                ("turbo", J::opt(self.live_turbo())),
                ("pl1W", J::opt(power.map(|p| p.0))),
                ("pl2W", J::opt(power.map(|p| p.1))),
                ("powerCustom", J::Bool(self.settings.cpu_power.is_some())),
                ("fanPreset", J::str(self.preset().id())),
                ("fanMode", J::opt(self.hw.read(&self.ec("fan_mode")))),
            ])),
            ("powerLimits", match limits {
                Some(l) => J::Obj(vec![
                    ("pl1Min", l.pl1_min.into()), ("pl1Max", l.pl1_max.into()),
                    ("pl2Min", l.pl2_min.into()), ("pl2Max", l.pl2_max.into()),
                ]),
                None => J::Null,
            }),
            ("fanCurve", match fan_tables {
                Some(t) => J::Obj(vec![
                    ("cpu", Self::curve_json(&t.cpu)),
                    ("gpu", if self.fan_layout().gpu { Self::curve_json(&t.gpu) } else { J::Null }),
                ]),
                None => J::Null,
            }),
            ("sensors", J::Obj(vec![
                // msi-ec reads the EC's own sensors; elsewhere use the standard ones.
                ("cpuTemp", if c.msi_ec { sensor("cpu", "realtime_temperature") } else { J::opt(caps::generic_cpu_temp(&self.hw)) }),
                ("gpuTemp", if c.msi_ec { sensor("gpu", "realtime_temperature") } else {
                    J::opt(self.shown_gpus().first().and_then(|g| self.gpu_live(g)).and_then(|l| l.temp_c))
                }),
                ("cpuClockMhz", J::opt(cpu::clock_mhz(&self.hw))),
                ("cpuPowerW", J::opt(self.cpu_power_w())),
                ("cpuFan", sensor("cpu", "realtime_fan_speed")),
                ("gpuFan", sensor("gpu", "realtime_fan_speed")),
                ("fans", J::Arr(self.fans().into_iter()
                    .map(|(label, rpm)| J::Obj(vec![("label", J::Str(label)), ("rpm", J::Int(rpm as i64))]))
                    .collect())),
            ])),
            ("battery", self.battery_json()),
            ("extras", self.extras_json()),
            ("gpus", J::Arr(self.shown_gpus().into_iter().map(|g| self.gpu_json(g)).collect())),
            ("dynamicBoost", if self.boost_available() {
                J::Obj(vec![("on", J::Bool(self.boost_on()))])
            } else {
                J::Null
            }),
        ]).render()
    }
}
