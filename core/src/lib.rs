//! FrostRig hardware core.
//!
//! Used by the `frostrigd` service, which runs as root. The desktop app
//! never touches hardware; it asks the service over D-Bus.

pub mod caps;
pub mod controller;
pub mod cpu;
pub mod fan;
pub mod gpu;
pub mod hw;
pub mod json;
pub mod state;

pub use controller::Controller;
pub use hw::Hw;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// End-of-charge limits offered in the app. msi-ec only accepts these three.
pub const CHARGE_LIMITS: [u8; 3] = [60, 80, 100];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Error(msg.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Silent,
    Balanced,
    Performance,
    Extreme,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Silent, Mode::Balanced, Mode::Performance, Mode::Extreme];

    pub fn id(self) -> &'static str {
        match self {
            Mode::Silent => "silent",
            Mode::Balanced => "balanced",
            Mode::Performance => "performance",
            Mode::Extreme => "extreme",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.id() == s)
    }

    /// msi-ec shift mode names, in order of preference.
    pub fn shift_names(self) -> &'static [&'static str] {
        match self {
            Mode::Silent => &["eco"],
            Mode::Balanced => &["comfort"],
            Mode::Performance => &["sport"],
            Mode::Extreme => &["turbo"],
        }
    }
}

#[cfg(test)]
mod tests;
