//! Live CPU readings: clock speed and package power draw. Read-only.

use crate::hw::Hw;

const CPU_DIR: &str = "/sys/devices/system/cpu";
const POWERCAP: &str = "/sys/class/powercap";

/// Average current clock of the online cores, in MHz. The busiest core jumps
/// around too much to be useful at a glance; the average follows load.
pub fn clock_mhz(hw: &Hw) -> Option<u32> {
    let khz: Vec<u64> = hw.list_dir(CPU_DIR).into_iter()
        .filter(|n| n.strip_prefix("cpu").is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
        .filter_map(|n| hw.read_u64(&format!("{CPU_DIR}/{n}/cpufreq/scaling_cur_freq")))
        .collect();
    if khz.is_empty() { return None; }
    Some((khz.iter().sum::<u64>() / khz.len() as u64 / 1000) as u32)
}

/// The RAPL package zone with an energy counter (Intel, and AMD Zen through
/// the same driver). Its `energy_uj` is readable by root only.
pub fn energy_zone(hw: &Hw) -> Option<String> {
    hw.list_dir(POWERCAP).into_iter()
        .filter(|z| z.starts_with("intel-rapl:") && z.matches(':').count() == 1)
        .map(|z| format!("{POWERCAP}/{z}"))
        .find(|d| hw.read(&format!("{d}/name")).is_some_and(|n| n.starts_with("package"))
            && hw.exists(&format!("{d}/energy_uj")))
}

/// Average watts between two readings of a RAPL energy counter, allowing for
/// the counter wrapping at `range` µJ. None when the readings don't make sense.
pub fn watts(before_uj: u64, after_uj: u64, range_uj: Option<u64>, secs: f64) -> Option<f64> {
    if secs <= 0.0 { return None; }
    let delta = if after_uj >= before_uj {
        after_uj - before_uj
    } else {
        range_uj?.checked_sub(before_uj)? + after_uj
    };
    Some(delta as f64 / 1_000_000.0 / secs)
}
