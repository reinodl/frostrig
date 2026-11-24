//! MSI fan tables: 7-point temperature and fan-speed blocks in the embedded
//! controller, CPU at 0x6A/0x72 and GPU at 0x82/0x8A. Verified on the Titan
//! GT77HX 13VI; other MSI models with the same layout are only used after an
//! on-device fan test (see Controller::start_fan_test). Some boards have only
//! the CPU curve; their GPU block is unused and never written.

use crate::caps::{EC_IO, MSI_EC};
use crate::hw::Hw;
use crate::{Error, Result};

pub const POINTS: usize = 7;
const CPU_TEMPS: u64 = 106;
const CPU_SPEEDS: u64 = 114;
const GPU_TEMPS: u64 = 130;
const GPU_SPEEDS: u64 = 138;

/// Refuse curve changes that reduce cooling when either chip is this hot.
pub const HOT_C: u64 = 85;

/// Which tables a laptop really uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// False on single-curve boards: one curve drives the fans and the GPU
    /// block is unused, so it's mirrored from the CPU curve and never written.
    pub gpu: bool,
}

pub const FULL: Layout = Layout { gpu: true };
pub const CPU_ONLY: Layout = Layout { gpu: false };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Curve {
    pub temps: [u8; POINTS],
    pub speeds: [u8; POINTS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tables {
    pub cpu: Curve,
    pub gpu: Curve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Auto,
    Quiet,
    Cool,
    Custom,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Auto, Preset::Quiet, Preset::Cool, Preset::Custom];

    pub fn id(self) -> &'static str {
        match self {
            Preset::Auto => "auto",
            Preset::Quiet => "quiet",
            Preset::Cool => "cool",
            Preset::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.id() == s)
    }
}

fn block(hw: &Hw, offset: u64) -> Result<[u8; POINTS]> {
    let v = hw.read_at(EC_IO, offset, POINTS)?;
    Ok(v.try_into().expect("read_at returns exactly POINTS bytes"))
}

pub fn read(hw: &Hw, layout: Layout) -> Result<Tables> {
    let cpu = Curve { temps: block(hw, CPU_TEMPS)?, speeds: block(hw, CPU_SPEEDS)? };
    let gpu = if layout.gpu { Curve { temps: block(hw, GPU_TEMPS)?, speeds: block(hw, GPU_SPEEDS)? } } else { cpu };
    Ok(Tables { cpu, gpu })
}

fn write_curves(hw: &Hw, t: &Tables, layout: Layout) -> Result<()> {
    hw.write_at(EC_IO, CPU_TEMPS, &t.cpu.temps)?;
    hw.write_at(EC_IO, CPU_SPEEDS, &t.cpu.speeds)?;
    if layout.gpu {
        hw.write_at(EC_IO, GPU_TEMPS, &t.gpu.temps)?;
        hw.write_at(EC_IO, GPU_SPEEDS, &t.gpu.speeds)?;
    }
    if read(hw, layout)? != *t {
        return Err(Error::new("Fan tables did not verify after writing"));
    }
    Ok(())
}

/// Write the tables the layout uses, each read back. On failure, put `before` back.
pub fn write(hw: &Hw, target: &Tables, before: &Tables, layout: Layout) -> Result<()> {
    let mut target = *target;
    if !layout.gpu { target.gpu = target.cpu; }
    match write_curves(hw, &target, layout) {
        Ok(()) => Ok(()),
        Err(e) => match write_curves(hw, before, layout) {
            Ok(()) => Err(Error::new(format!("{e}. The previous fan curve was put back."))),
            Err(e2) => Err(Error::new(format!(
                "{e}. Putting the previous fan curve back ALSO failed ({e2}). Turn on Max Fans and restart."
            ))),
        },
    }
}

/// Raw bytes for the system report: `len` bytes from `from`, 16 per line.
pub fn dump(hw: &Hw, from: u64, len: usize) -> Option<String> {
    let bytes = hw.read_at(EC_IO, from, len).ok()?;
    Some(bytes.chunks(16).enumerate().map(|(i, row)| {
        format!("{:02X}: {}", from as usize + i * 16, row.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "))
    }).collect::<Vec<_>>().join("\n"))
}

/// Does this block look like a 7-point fan curve? Temperatures rising
/// within 30–110 °C, speeds never falling and ending at 70% or more.
pub fn curve_plausible(c: &Curve) -> bool {
    c.temps.iter().all(|&x| (30..=110).contains(&x)) && c.temps.windows(2).all(|w| w[0] < w[1])
        && c.speeds.iter().all(|&x| x <= 150) && c.speeds.windows(2).all(|w| w[0] <= w[1])
        && c.speeds[POINTS - 1] >= 70
}

/// The live tables must still look like this layout before anything is written.
pub fn plausible(t: &Tables, layout: Layout) -> Result<()> {
    let mut parts = vec![("CPU", &t.cpu)];
    if layout.gpu { parts.push(("GPU", &t.gpu)); }
    for (name, c) in parts {
        if !curve_plausible(c) {
            return Err(Error::new(format!(
                "The {name} fan table doesn't match this laptop's known layout, so it was left alone."
            )));
        }
    }
    Ok(())
}

/// Rules for a curve the user drew.
pub fn validate_custom(t: &Tables) -> Result<()> {
    for (name, c) in [("CPU", &t.cpu), ("GPU", &t.gpu)] {
        if !c.temps.iter().all(|&x| (30..=100).contains(&x)) {
            return Err(Error::new(format!("{name}: temperatures must be between 30 and 100 °C.")));
        }
        if !c.speeds.iter().all(|&x| (30..=100).contains(&x)) {
            return Err(Error::new(format!("{name}: fan speeds must be between 30 and 100%.")));
        }
        if !c.temps.windows(2).all(|w| w[0] < w[1]) {
            return Err(Error::new(format!("{name}: each point must be hotter than the one before.")));
        }
        if !c.speeds.windows(2).all(|w| w[0] <= w[1]) {
            return Err(Error::new(format!("{name}: fan speed can't drop as temperature rises.")));
        }
        if c.temps[POINTS - 1] != 100 || c.speeds[POINTS - 1] != 100 {
            return Err(Error::new(format!("{name}: the last point must be 100 °C at 100%.")));
        }
    }
    Ok(())
}

/// Ready-made curves are derived from the laptop's own factory tables,
/// never from invented numbers.
pub fn preset(factory: &Tables, p: Preset) -> Option<Tables> {
    let map = |c: &Curve, f: &dyn Fn(u8, u8) -> u8| {
        let mut out = *c;
        for i in 0..POINTS {
            out.speeds[i] = f(c.temps[i], c.speeds[i]);
        }
        // Keep speeds non-decreasing after the adjustment.
        for i in 1..POINTS {
            out.speeds[i] = out.speeds[i].max(out.speeds[i - 1]);
        }
        out
    };
    match p {
        Preset::Auto => Some(*factory),
        // Quieter only below 70 °C; the hot end of the curve stays factory.
        Preset::Quiet => {
            let f = |t: u8, s: u8| if t < 70 { s.saturating_sub(10).max(30.min(s)) } else { s };
            Some(Tables { cpu: map(&factory.cpu, &f), gpu: map(&factory.gpu, &f) })
        }
        // Ten points more fan everywhere, never above 100% unless factory already was.
        Preset::Cool => {
            let f = |_t: u8, s: u8| s.max(s.saturating_add(10).min(100));
            Some(Tables { cpu: map(&factory.cpu, &f), gpu: map(&factory.gpu, &f) })
        }
        Preset::Custom => None,
    }
}

/// msi-ec's live fan speed in percent, for "cpu" or "gpu".
pub fn fan_speed(hw: &Hw, part: &str) -> Option<u64> {
    hw.read_u64(&format!("{MSI_EC}/{part}/realtime_fan_speed"))
}

pub fn hottest(hw: &Hw) -> Option<u64> {
    let cpu = hw.read_u64(&format!("{MSI_EC}/cpu/realtime_temperature"));
    let gpu = hw.read_u64(&format!("{MSI_EC}/gpu/realtime_temperature"));
    cpu.into_iter().chain(gpu).max()
}

pub fn encode(c: &[u8; POINTS]) -> String {
    c.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
}

pub fn decode(s: &str) -> Option<[u8; POINTS]> {
    let v: Vec<u8> = s.split(',').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
    v.try_into().ok()
}
