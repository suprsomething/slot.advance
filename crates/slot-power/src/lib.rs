mod device;
mod lid;
mod motor;
mod power;
mod quiet;
mod sim;
mod sleep;
mod unload;

pub use device::{
    has_bit, motor_change, read_only, read_only_in, record_first_frame, rumble_node,
    trace_first_frame, uptime_seconds, DevicePlatform,
};
pub use lid::{raw_level, LidReading, LidSensor};
pub use motor::Motor;
pub use power::Power;
pub use quiet::{quiet_hardware, Quiet, QuietReport, StaleCard, WifiResult};
pub use sim::SimPlatform;
pub use sleep::{SleepPaths, Woke};
pub use unload::{DriverUnload, Unload, UNLOAD_LIMIT, WIFI_DRIVER};

use std::path::Path;
use std::time::Duration;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Charge {
    Unknown,
    Discharging,
    Charging,
    Full,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Battery {
    pub percent: u8,
    pub charge: Charge,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum LedState {
    Off,
    Running,
    Low,
    Charging,
    Charged,
}

pub trait Platform: Send {
    fn set_backlight(&mut self, step: u8);
    fn battery(&self) -> Option<Battery>;
    fn charge(&self) -> Charge;
    fn set_led(&mut self, state: LedState);
    fn poweroff(&mut self) -> !;
    fn restart(&mut self) -> !;
    fn root(&self) -> &Path;
    fn now(&self) -> i64;
    fn set_clock(&mut self, secs: i64);
    fn set_rumble(&mut self, strength: u16);

    fn relink_adb(&mut self) -> bool {
        false
    }

    fn headphones(&self) -> bool {
        false
    }

    fn card_read_only(&self) -> bool {
        false
    }

    /// Whether genuine suspend-to-RAM with lid wake is available. Platforms without it
    /// keep the software standby of v3.
    fn sleep_ready(&self) -> bool {
        false
    }

    /// Suspends to RAM with the timer set `secs` ahead, and returns once the device is
    /// awake again. Only called when `sleep_ready` says so.
    fn suspend(&mut self, _secs: u32) -> Woke {
        Woke::refused("this platform has no hardware sleep")
    }

    /// Seconds since boot, counting time spent suspended.
    fn uptime(&self) -> Option<f64> {
        None
    }

    /// The lid as the hardware reports it now, if it can be read.
    fn lid_open(&self) -> Option<bool> {
        None
    }
}

pub trait LidPolicy {
    fn on_close(&mut self);
    fn on_open(&mut self);
    fn timeout(&self) -> Duration;
}
