//! GPUs from any vendor: detection, live readings, and the controls each
//! driver really allows on this machine.
//!
//! Rules every backend follows:
//! - A GPU that's runtime-suspended (hybrid laptops) is never touched: reading
//!   its driver files or running nvidia-smi would wake it and drain the battery.
//! - A control is only offered after the driver has accepted a harmless test
//!   write of the value it already has ("probe, verify, present").
//! - Driver settings are lost when the GPU powers off, so the controller
//!   re-applies them whenever a GPU wakes.
//!
//! Backends: NVIDIA (proprietary/open driver via nvidia-smi), AMD (amdgpu
//! sysfs), Intel (i915 and xe sysfs). Anything else is listed without controls.

use crate::hw::Hw;
use crate::{Error, Result};
use std::process::Command;

pub const NVIDIA_SMI: &str = "/usr/bin/nvidia-smi";
pub const SYSTEMCTL: &str = "/usr/bin/systemctl";
pub const POWERD_UNIT: &str = "nvidia-powerd.service";
pub const POWERD_UNIT_FILE: &str = "/usr/lib/systemd/system/nvidia-powerd.service";
const PCI: &str = "/sys/bus/pci/devices";

/// Lowest NVIDIA clock cap offered: below this the desktop gets sluggish.
pub const NVIDIA_MIN_CLOCK_MHZ: u32 = 600;
pub const NVIDIA_CLOCK_STEP_MHZ: u32 = 15;
pub const INTEL_CLOCK_STEP_MHZ: u32 = 50;
/// Intel Arc power caps are offered from this share of the rated maximum up to it.
const INTEL_POWER_FLOOR: f64 = 0.5;

/// Runs a program and returns its stdout, or an error if it failed.
/// Swapped for a fake in tests.
pub trait Runner: Send {
    fn run(&self, program: &str, args: &[&str]) -> Result<String>;
}

pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let out = Command::new(program).args(args).output()
            .map_err(|e| Error::new(format!("Could not run {program}: {e}")))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let msg = if stderr.trim().is_empty() { stdout.trim().to_owned() } else { stderr.trim().to_owned() };
            Err(Error::new(msg))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
}

impl Vendor {
    pub fn id(self) -> &'static str {
        match self {
            Vendor::Nvidia => "nvidia",
            Vendor::Amd => "amd",
            Vendor::Intel => "intel",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    /// PCI address, e.g. 0000:01:00.0. Used as the GPU's id everywhere.
    pub pci: String,
    pub vendor: Vendor,
    /// Kernel driver bound to it: nvidia, amdgpu, i915, xe, nouveau, radeon, ...
    pub driver: String,
    pub integrated: bool,
    /// The GPU's hwmon directory, if its driver has one.
    pub hwmon: Option<String>,
}

impl Gpu {
    pub fn dev(&self) -> String {
        format!("{PCI}/{}", self.pci)
    }

    /// Which control backend applies: needs the vendor's own driver.
    pub fn backend(&self) -> Option<Vendor> {
        match (self.vendor, self.driver.as_str()) {
            (Vendor::Nvidia, "nvidia") => Some(Vendor::Nvidia),
            (Vendor::Amd, "amdgpu") => Some(Vendor::Amd),
            (Vendor::Intel, "i915" | "xe") => Some(Vendor::Intel),
            _ => None,
        }
    }
}

/// Every display controller on the PCI bus from NVIDIA, AMD or Intel.
pub fn detect_all(hw: &Hw) -> Vec<Gpu> {
    let mut gpus = Vec::new();
    for pci in hw.list_dir(PCI) {
        let dev = format!("{PCI}/{pci}");
        if !hw.read(&format!("{dev}/class")).is_some_and(|c| c.starts_with("0x03")) {
            continue;
        }
        let vendor = match hw.read(&format!("{dev}/vendor")).as_deref() {
            Some("0x10de") => Vendor::Nvidia,
            Some("0x1002") => Vendor::Amd,
            Some("0x8086") => Vendor::Intel,
            _ => continue,
        };
        let Some(driver) = hw.link_name(&format!("{dev}/driver")) else { continue };
        let integrated = match vendor {
            // Intel's built-in graphics always sits at 00:02.0; Arc cards don't.
            Vendor::Intel => pci.ends_with(":00:02.0"),
            // AMD APUs carve out a small slice of RAM as "VRAM"; real cards have 4 GiB or more.
            Vendor::Amd => hw.read_u64(&format!("{dev}/mem_info_vram_total")).is_some_and(|b| b < 3 << 30),
            Vendor::Nvidia => false,
        };
        let hwmon = hw.list_dir(&format!("{dev}/hwmon")).into_iter().next().map(|h| format!("{dev}/hwmon/{h}"));
        gpus.push(Gpu { pci, vendor, driver, integrated, hwmon });
    }
    // Dedicated GPUs first.
    gpus.sort_by_key(|g| g.integrated);
    gpus
}

/// Whether the GPU is powered on. Reading this never wakes it.
pub fn awake(hw: &Hw, gpu: &Gpu) -> bool {
    !matches!(
        hw.read(&format!("{}/power/runtime_status", gpu.dev())).as_deref(),
        Some("suspended") | Some("suspending")
    )
}

// ---------------------------------------------------------------- readings

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub name: Option<String>,
    pub temp_c: Option<u32>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub clock_mhz: Option<u32>,
    pub max_clock_mhz: Option<u32>,
}

fn num(f: &[&str], i: usize) -> Option<f64> {
    f.get(i).and_then(|v| v.parse::<f64>().ok())
}

/// `nvidia-smi --query-gpu=name,power.draw,enforced.power.limit,clocks.gr,clocks.max.graphics,temperature.gpu`.
/// Fields the driver can't report read "[N/A]".
pub fn parse_nvidia_live(line: &str) -> Live {
    let f: Vec<&str> = line.lines().next().unwrap_or("").split(',').map(str::trim).collect();
    Live {
        name: f.first().filter(|s| !s.is_empty() && !s.starts_with('[')).map(|s| (*s).to_owned()),
        power_w: num(&f, 1),
        power_limit_w: num(&f, 2),
        clock_mhz: num(&f, 3).map(|v| v as u32),
        max_clock_mhz: num(&f, 4).map(|v| v as u32),
        temp_c: num(&f, 5).map(|v| v as u32),
    }
}

fn intel_freq_dir(hw: &Hw, gpu: &Gpu) -> Option<(String, &'static str)> {
    // i915: cardN/gt_*_freq_mhz.  xe: device/tile0/gt0/freq0/*_freq.
    let drm = hw.list_dir(&format!("{}/drm", gpu.dev())).into_iter().find(|c| c.starts_with("card") && !c.contains('-'))?;
    let i915 = format!("{}/drm/{drm}", gpu.dev());
    if hw.exists(&format!("{i915}/gt_max_freq_mhz")) {
        return Some((i915, "i915"));
    }
    let xe = format!("{}/tile0/gt0/freq0", gpu.dev());
    if hw.exists(&format!("{xe}/max_freq")) {
        return Some((xe, "xe"));
    }
    None
}

fn intel_freq_file(kind: &str, which: &str) -> String {
    match (kind, which) {
        ("i915", "act") => "gt_act_freq_mhz".into(),
        ("i915", "max") => "gt_max_freq_mhz".into(),
        ("i915", "min") => "gt_min_freq_mhz".into(),
        ("i915", "rp0") => "gt_RP0_freq_mhz".into(),
        ("i915", "rpn") => "gt_RPn_freq_mhz".into(),
        (_, w) => format!("{w}_freq"),
    }
}

/// Live readings. Never call for a sleeping GPU (see `awake`).
pub fn read_live(hw: &Hw, run: &dyn Runner, gpu: &Gpu) -> Live {
    match gpu.backend() {
        Some(Vendor::Nvidia) => run.run(NVIDIA_SMI, &[
            "-i", &gpu.pci,
            "--query-gpu=name,power.draw,enforced.power.limit,clocks.gr,clocks.max.graphics,temperature.gpu",
            "--format=csv,noheader,nounits",
        ]).map(|o| parse_nvidia_live(&o)).unwrap_or_default(),
        _ => {
            let h = gpu.hwmon.clone().unwrap_or_default();
            let uw = |f: &str| hw.read_u64(&format!("{h}/{f}")).map(|v| v as f64 / 1e6);
            let mut live = Live {
                name: hw.read(&format!("{}/product_name", gpu.dev())),
                temp_c: hw.read_u64(&format!("{h}/temp1_input")).map(|m| (m / 1000) as u32),
                power_w: uw("power1_average").or_else(|| uw("power1_input")),
                power_limit_w: uw("power1_cap").or_else(|| uw("power1_max")),
                clock_mhz: hw.read_u64(&format!("{h}/freq1_input")).map(|hz| (hz / 1_000_000) as u32),
                max_clock_mhz: None,
            };
            if let Some((dir, kind)) = intel_freq_dir(hw, gpu) {
                live.clock_mhz = hw.read_u64(&format!("{dir}/{}", intel_freq_file(kind, "act"))).map(|v| v as u32);
                live.max_clock_mhz = hw.read_u64(&format!("{dir}/{}", intel_freq_file(kind, "rp0"))).map(|v| v as u32);
            }
            live
        }
    }
}

pub fn default_name(gpu: &Gpu) -> String {
    match (gpu.vendor, gpu.integrated) {
        (Vendor::Nvidia, _) => "NVIDIA GPU".into(),
        (Vendor::Amd, true) => "AMD Radeon Graphics".into(),
        (Vendor::Amd, false) => "AMD Radeon GPU".into(),
        (Vendor::Intel, true) => "Intel Graphics".into(),
        (Vendor::Intel, false) => "Intel Arc GPU".into(),
    }
}

// ---------------------------------------------------------------- clock cap

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockRange {
    pub min: u32,
    pub max: u32,
    pub step: u32,
}

/// Clock cap range from sysfs (Intel). NVIDIA's comes from the learned top clock.
pub fn intel_clock_range(hw: &Hw, gpu: &Gpu) -> Option<ClockRange> {
    let (dir, kind) = intel_freq_dir(hw, gpu)?;
    let rd = |w: &str| hw.read_u64(&format!("{dir}/{}", intel_freq_file(kind, w))).map(|v| v as u32);
    let (rpn, rp0) = (rd("rpn")?, rd("rp0")?);
    // The cap can't go below the current minimum, or the driver refuses it.
    let floor = rd("min").unwrap_or(rpn).max(rpn);
    (rp0 > floor).then_some(ClockRange { min: floor, max: rp0, step: INTEL_CLOCK_STEP_MHZ })
}

/// Current Intel cap (None = at the hardware maximum).
pub fn intel_clock_cap(hw: &Hw, gpu: &Gpu) -> Option<u32> {
    let (dir, kind) = intel_freq_dir(hw, gpu)?;
    let max = hw.read_u64(&format!("{dir}/{}", intel_freq_file(kind, "max")))? as u32;
    let rp0 = hw.read_u64(&format!("{dir}/{}", intel_freq_file(kind, "rp0")))? as u32;
    (max < rp0).then_some(max)
}

/// Cap the clock (Some) or remove the cap (None).
pub fn set_clock_cap(hw: &Hw, run: &dyn Runner, gpu: &Gpu, mhz: Option<u32>) -> Result<()> {
    match gpu.backend() {
        Some(Vendor::Nvidia) => {
            let r = match mhz {
                Some(m) => run.run(NVIDIA_SMI, &["-i", &gpu.pci, "-lgc", &format!("0,{m}")]),
                None => run.run(NVIDIA_SMI, &["-i", &gpu.pci, "-rgc"]),
            };
            r.map(|_| ()).map_err(|e| Error::new(format!("The NVIDIA driver refused the clock limit: {e}")))
        }
        Some(Vendor::Intel) => {
            let (dir, kind) = intel_freq_dir(hw, gpu).ok_or_else(|| Error::new("No Intel GPU frequency controls found."))?;
            let rp0 = hw.read_u64(&format!("{dir}/{}", intel_freq_file(kind, "rp0"))).unwrap_or(0) as u32;
            let target = mhz.unwrap_or(rp0);
            let path = format!("{dir}/{}", intel_freq_file(kind, "max"));
            hw.write_raw(&path, &target.to_string())?;
            // The driver rounds to its own frequency steps.
            match hw.read_u64(&path) {
                Some(v) if (v as u32).abs_diff(target) <= INTEL_CLOCK_STEP_MHZ => Ok(()),
                v => Err(Error::new(format!("Intel GPU max frequency reads back {v:?} after writing {target}"))),
            }
        }
        _ => Err(Error::new("This GPU's clock can't be limited.")),
    }
}

// ---------------------------------------------------------------- power limit

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerRange {
    pub min_w: u32,
    pub max_w: u32,
    pub default_w: u32,
}

/// What the driver reports about its power limit, before any verification.
/// NVIDIA: query.  AMD: hwmon power1_cap*.  Intel: hwmon power1_max + rated max.
pub fn power_info(hw: &Hw, run: &dyn Runner, gpu: &Gpu) -> Option<(PowerRange, u32)> {
    let w = |uw: u64| (uw / 1_000_000) as u32;
    match gpu.backend()? {
        Vendor::Nvidia => {
            let out = run.run(NVIDIA_SMI, &[
                "-i", &gpu.pci,
                "--query-gpu=power.limit,power.default_limit,power.min_limit,power.max_limit",
                "--format=csv,noheader,nounits",
            ]).ok()?;
            let f: Vec<&str> = out.lines().next()?.split(',').map(str::trim).collect();
            // Laptops that lock the limit report the current value as [N/A].
            let cur = num(&f, 0)? as u32;
            let (def, min, max) = (num(&f, 1)? as u32, num(&f, 2)? as u32, num(&f, 3)? as u32);
            (max > min).then_some((PowerRange { min_w: min, max_w: max, default_w: def }, cur))
        }
        Vendor::Amd => {
            let h = gpu.hwmon.as_ref()?;
            let cur = hw.read_u64(&format!("{h}/power1_cap"))?;
            let max = hw.read_u64(&format!("{h}/power1_cap_max"))?;
            let min = hw.read_u64(&format!("{h}/power1_cap_min")).unwrap_or(0).max(1_000_000);
            let def = hw.read_u64(&format!("{h}/power1_cap_default")).unwrap_or(cur);
            (max > min).then_some((PowerRange { min_w: w(min), max_w: w(max), default_w: w(def) }, w(cur)))
        }
        Vendor::Intel => {
            let h = gpu.hwmon.as_ref()?;
            let cur = hw.read_u64(&format!("{h}/power1_max"))?;
            let rated = hw.read_u64(&format!("{h}/power1_rated_max"))?;
            let min = (rated as f64 * INTEL_POWER_FLOOR) as u64;
            (rated > min && rated > 0).then_some((PowerRange { min_w: w(min), max_w: w(rated), default_w: w(rated) }, w(cur)))
        }
    }
}

const NVIDIA_REFUSALS: [&str; 5] = ["not supported", "not allowed", "insufficient permissions", "error", "failed"];

/// Set the power limit in watts. NVIDIA "succeeds" even when it refuses
/// (exit code 0 with "not supported"), so its text is checked too.
pub fn set_power(hw: &Hw, run: &dyn Runner, gpu: &Gpu, watts: u32) -> Result<()> {
    match gpu.backend() {
        Some(Vendor::Nvidia) => {
            let out = run.run(NVIDIA_SMI, &["-i", &gpu.pci, "-pl", &watts.to_string()])
                .map_err(|e| Error::new(format!("The NVIDIA driver refused the power limit: {e}")))?;
            let lower = out.to_lowercase();
            if NVIDIA_REFUSALS.iter().any(|r| lower.contains(r)) {
                return Err(Error::new(format!("The NVIDIA driver refused the power limit: {}", out.trim())));
            }
            Ok(())
        }
        Some(Vendor::Amd) | Some(Vendor::Intel) => {
            let h = gpu.hwmon.as_ref().ok_or_else(|| Error::new("This GPU has no power controls."))?;
            let file = if gpu.backend() == Some(Vendor::Amd) { "power1_cap" } else { "power1_max" };
            let path = format!("{h}/{file}");
            let uw = watts as u64 * 1_000_000;
            hw.write_raw(&path, &uw.to_string())?;
            match hw.read_u64(&path) {
                Some(v) if v.abs_diff(uw) <= 1_000_000 => Ok(()),
                v => Err(Error::new(format!("GPU power limit reads back {v:?} µW after writing {uw} µW"))),
            }
        }
        None => Err(Error::new("This GPU's power limit can't be changed.")),
    }
}

// ---------------------------------------------------------------- AMD performance level

/// Levels FrostRig offers, as amdgpu names them.
pub const AMD_LEVELS: [&str; 3] = ["auto", "low", "high"];

pub fn amd_level_file(gpu: &Gpu) -> String {
    format!("{}/power_dpm_force_performance_level", gpu.dev())
}

pub fn amd_level(hw: &Hw, gpu: &Gpu) -> Option<String> {
    (gpu.backend() == Some(Vendor::Amd)).then(|| hw.read(&amd_level_file(gpu))).flatten()
}

pub fn set_amd_level(hw: &Hw, gpu: &Gpu, level: &str) -> Result<()> {
    if !AMD_LEVELS.contains(&level) {
        return Err(Error::new(format!("Unknown GPU performance level {level:?}.")));
    }
    if gpu.backend() != Some(Vendor::Amd) {
        return Err(Error::new("Performance levels are only available on AMD GPUs."));
    }
    hw.write_verified(&amd_level_file(gpu), level)
}

// ---------------------------------------------------------------- NVIDIA Dynamic Boost

pub fn boost_running(run: &dyn Runner) -> bool {
    run.run(SYSTEMCTL, &["is-active", POWERD_UNIT]).is_ok_and(|s| s.trim() == "active")
}

/// Whether nvidia-powerd starts at boot on its own (the system's default).
pub fn boost_enabled_by_system(run: &dyn Runner) -> bool {
    run.run(SYSTEMCTL, &["is-enabled", POWERD_UNIT]).is_ok_and(|s| s.trim() == "enabled")
}

pub fn set_boost(run: &dyn Runner, on: bool) -> Result<()> {
    run.run(SYSTEMCTL, &[if on { "start" } else { "stop" }, POWERD_UNIT])
        .map(|_| ())
        .map_err(|e| Error::new(format!("Could not {} Dynamic Boost: {e}", if on { "start" } else { "stop" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_smi_query_output() {
        let l = parse_nvidia_live("NVIDIA GeForce RTX 4090 Laptop GPU, 17.92, 150.00, 855, 3105, 41\n");
        assert_eq!(l.name.as_deref(), Some("NVIDIA GeForce RTX 4090 Laptop GPU"));
        assert_eq!(l.power_w, Some(17.92));
        assert_eq!(l.power_limit_w, Some(150.0));
        assert_eq!(l.clock_mhz, Some(855));
        assert_eq!(l.max_clock_mhz, Some(3105));
        assert_eq!(l.temp_c, Some(41));
        let na = parse_nvidia_live("NVIDIA GeForce RTX 4090 Laptop GPU, [N/A], 150.00, 855, 3105, 41");
        assert_eq!(na.power_w, None);
    }
}
