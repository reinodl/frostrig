//! What the user chose (restored at boot and after sleep) and what the laptop
//! shipped with (used for "Reset" and as the ceiling for power limits).
//! Stored as plain `key=value` lines.

use crate::fan::{self, Curve, Preset, Tables};
use crate::Mode;
use std::collections::BTreeMap;

pub const STATE_DIR: &str = "/var/lib/frostrig";
pub const SETTINGS_FILE: &str = "/var/lib/frostrig/settings";
pub const FACTORY_FILE: &str = "/var/lib/frostrig/factory";

fn parse(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') { return None; }
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_owned(), v.trim().to_owned()))
        })
        .collect()
}

fn bool_of(s: &str) -> Option<bool> {
    match s { "1" | "true" => Some(true), "0" | "false" => Some(false), _ => None }
}

fn curve_of(m: &BTreeMap<String, String>, prefix: &str) -> Option<Curve> {
    Some(Curve {
        temps: fan::decode(m.get(&format!("{prefix}_temps"))?)?,
        speeds: fan::decode(m.get(&format!("{prefix}_speeds"))?)?,
    })
}

fn tables_of(m: &BTreeMap<String, String>, prefix: &str) -> Option<Tables> {
    Some(Tables { cpu: curve_of(m, &format!("{prefix}_cpu"))?, gpu: curve_of(m, &format!("{prefix}_gpu"))? })
}

fn push_tables(out: &mut String, prefix: &str, t: &Tables) {
    for (part, c) in [("cpu", &t.cpu), ("gpu", &t.gpu)] {
        out.push_str(&format!("{prefix}_{part}_temps={}\n", fan::encode(&c.temps)));
        out.push_str(&format!("{prefix}_{part}_speeds={}\n", fan::encode(&c.speeds)));
    }
}

/// Per-GPU keys look like `gpu.0000:01:00.0.clock_mhz`; the PCI address
/// itself contains dots, so split on the last one.
fn per_gpu(m: &BTreeMap<String, String>) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (k, v) in m {
        if let Some((pci, field)) = k.strip_prefix("gpu.").and_then(|r| r.rsplit_once('.')) {
            out.entry(pci.to_owned()).or_default().insert(field.to_owned(), v.clone());
        }
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuSettings {
    /// Clock cap in MHz. None = no cap.
    pub clock_mhz: Option<u32>,
    /// Power limit in watts. None = driver default.
    pub power_w: Option<u32>,
    /// AMD performance level ("auto", "low", "high"). None = untouched.
    pub level: Option<String>,
}

impl GpuSettings {
    pub fn is_empty(&self) -> bool {
        self.clock_mhz.is_none() && self.power_w.is_none() && self.level.is_none()
    }
}

/// Only settings the user has actually changed are stored and restored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub mode: Option<Mode>,
    pub cooler_boost: Option<bool>,
    pub webcam: Option<bool>,
    pub webcam_block: Option<bool>,
    /// "left" or "right": which side of the Win key the Fn key is on.
    pub fn_key: Option<String>,
    pub super_battery: Option<bool>,
    pub kbd_backlight: Option<u64>,
    pub charge_limit: Option<u8>,
    pub turbo: Option<bool>,
    /// (PL1, PL2) in watts. None = firmware default.
    pub cpu_power: Option<(u32, u32)>,
    pub fan_preset: Option<Preset>,
    pub custom_curve: Option<Tables>,
    /// Per GPU, keyed by PCI address.
    pub gpus: BTreeMap<String, GpuSettings>,
    /// `gpu_clock_limit_mhz` from v0.16 (one NVIDIA GPU, no address).
    /// Moved onto the NVIDIA GPU when it's found.
    pub legacy_gpu_clock: Option<u32>,
    pub dynamic_boost: Option<bool>,
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let m = parse(text);
        let g = |k: &str| m.get(k).map(String::as_str);
        Settings {
            mode: g("mode").and_then(Mode::parse),
            cooler_boost: g("cooler_boost").and_then(bool_of),
            webcam: g("webcam").and_then(bool_of),
            webcam_block: g("webcam_block").and_then(bool_of),
            fn_key: g("fn_key").filter(|v| *v == "left" || *v == "right").map(str::to_owned),
            super_battery: g("super_battery").and_then(bool_of),
            kbd_backlight: g("kbd_backlight").and_then(|v| v.parse().ok()),
            charge_limit: g("charge_limit").and_then(|v| v.parse().ok()).filter(|v| crate::CHARGE_LIMITS.contains(v)),
            turbo: g("turbo").and_then(bool_of),
            cpu_power: match (g("pl1_w").and_then(|v| v.parse().ok()), g("pl2_w").and_then(|v| v.parse().ok())) {
                (Some(a), Some(b)) => Some((a, b)),
                _ => None,
            },
            fan_preset: g("fan_preset").and_then(Preset::parse),
            custom_curve: tables_of(&m, "custom"),
            gpus: per_gpu(&m).into_iter().map(|(pci, f)| (pci, GpuSettings {
                clock_mhz: f.get("clock_mhz").and_then(|v| v.parse().ok()),
                power_w: f.get("power_w").and_then(|v| v.parse().ok()),
                level: f.get("level").cloned(),
            })).filter(|(_, g)| !g.is_empty()).collect(),
            legacy_gpu_clock: g("gpu_clock_limit_mhz").and_then(|v| v.parse().ok()),
            dynamic_boost: g("dynamic_boost").and_then(bool_of),
        }
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("# FrostRig settings. Written by frostrigd; edit with the app.\n");
        if let Some(v) = self.mode { out.push_str(&format!("mode={}\n", v.id())); }
        if let Some(v) = self.cooler_boost { out.push_str(&format!("cooler_boost={}\n", v as u8)); }
        if let Some(v) = self.webcam { out.push_str(&format!("webcam={}\n", v as u8)); }
        if let Some(v) = self.webcam_block { out.push_str(&format!("webcam_block={}\n", v as u8)); }
        if let Some(v) = &self.fn_key { out.push_str(&format!("fn_key={v}\n")); }
        if let Some(v) = self.super_battery { out.push_str(&format!("super_battery={}\n", v as u8)); }
        if let Some(v) = self.kbd_backlight { out.push_str(&format!("kbd_backlight={v}\n")); }
        if let Some(v) = self.charge_limit { out.push_str(&format!("charge_limit={v}\n")); }
        if let Some(v) = self.turbo { out.push_str(&format!("turbo={}\n", v as u8)); }
        if let Some((a, b)) = self.cpu_power { out.push_str(&format!("pl1_w={a}\npl2_w={b}\n")); }
        if let Some(v) = self.fan_preset { out.push_str(&format!("fan_preset={}\n", v.id())); }
        if let Some(t) = &self.custom_curve { push_tables(&mut out, "custom", t); }
        for (pci, g) in &self.gpus {
            if let Some(v) = g.clock_mhz { out.push_str(&format!("gpu.{pci}.clock_mhz={v}\n")); }
            if let Some(v) = g.power_w { out.push_str(&format!("gpu.{pci}.power_w={v}\n")); }
            if let Some(v) = &g.level { out.push_str(&format!("gpu.{pci}.level={v}\n")); }
        }
        if let Some(v) = self.legacy_gpu_clock { out.push_str(&format!("gpu_clock_limit_mhz={v}\n")); }
        if let Some(v) = self.dynamic_boost { out.push_str(&format!("dynamic_boost={}\n", v as u8)); }
        out
    }
}

/// Captured once, the first time the service runs, before it changes anything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Factory {
    pub pl1_uw: Option<u64>,
    pub pl2_uw: Option<u64>,
    pub turbo: Option<bool>,
    pub fan_tables: Option<Tables>,
    /// EC firmware on which the fan test passed, and whether the GPU curve
    /// is real (false = single-curve board).
    pub fan_verified_fw: Option<String>,
    pub fan_verified_gpu: Option<bool>,
    /// Highest clock each NVIDIA GPU reports, learned the first time it's awake.
    pub gpu_max_clock: BTreeMap<String, u32>,
    /// From v0.16, before per-GPU keys.
    pub legacy_gpu_max_clock: Option<u32>,
}

impl Factory {
    pub fn parse(text: &str) -> Factory {
        let m = parse(text);
        Factory {
            pl1_uw: m.get("pl1_uw").and_then(|v| v.parse().ok()),
            pl2_uw: m.get("pl2_uw").and_then(|v| v.parse().ok()),
            turbo: m.get("turbo").and_then(|v| bool_of(v)),
            fan_tables: tables_of(&m, "factory"),
            fan_verified_fw: m.get("fan_verified_fw").cloned(),
            fan_verified_gpu: m.get("fan_verified_gpu").and_then(|v| bool_of(v)),
            gpu_max_clock: per_gpu(&m).into_iter()
                .filter_map(|(pci, f)| Some((pci, f.get("max_clock_mhz")?.parse().ok()?)))
                .collect(),
            legacy_gpu_max_clock: m.get("gpu_max_clock_mhz").and_then(|v| v.parse().ok()),
        }
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("# Laptop defaults captured by frostrigd before it changed anything.\n");
        if let Some(v) = self.pl1_uw { out.push_str(&format!("pl1_uw={v}\n")); }
        if let Some(v) = self.pl2_uw { out.push_str(&format!("pl2_uw={v}\n")); }
        if let Some(v) = self.turbo { out.push_str(&format!("turbo={}\n", v as u8)); }
        if let Some(t) = &self.fan_tables { push_tables(&mut out, "factory", t); }
        if let Some(v) = &self.fan_verified_fw { out.push_str(&format!("fan_verified_fw={v}\n")); }
        if let Some(v) = self.fan_verified_gpu { out.push_str(&format!("fan_verified_gpu={}\n", v as u8)); }
        for (pci, v) in &self.gpu_max_clock { out.push_str(&format!("gpu.{pci}.max_clock_mhz={v}\n")); }
        if let Some(v) = self.legacy_gpu_max_clock { out.push_str(&format!("gpu_max_clock_mhz={v}\n")); }
        out
    }
}
