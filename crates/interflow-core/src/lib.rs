pub mod platform;
pub mod traits;
pub mod device;
pub mod stream;
pub mod proxies;
pub mod timing;
pub mod buffer;

use bitflags::bitflags;

bitflags! {
    /// Represents the types/capabilities of an audio device.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DeviceType: u32 {
        /// Device supports audio input.
        const INPUT = 1 << 0;

        /// Device supports audio output.
        const OUTPUT = 1 << 1;

        /// Physical audio device (hardware).
        const PHYSICAL = 1 << 2;

        /// Virtual/software application device.
        const APPLICATION = 1 << 3;

        /// This device is set as default
        const DEFAULT = 1 << 4;

        /// Device that supports both input and output.
        const DUPLEX = Self::INPUT.bits() | Self::OUTPUT.bits();
    }
}

impl DeviceType {
    /// Returns true if this device type has the input capability.
    pub fn is_input(&self) -> bool {
        self.contains(Self::INPUT)
    }

    /// Returns true if this device type has the output capability.
    pub fn is_output(&self) -> bool {
        self.contains(Self::OUTPUT)
    }

    /// Returns true if this device type is a physical device.
    pub fn is_physical(&self) -> bool {
        self.contains(Self::PHYSICAL)
    }

    /// Returns true if this device type is an application/virtual device.
    pub fn is_application(&self) -> bool {
        self.contains(Self::APPLICATION)
    }

    /// Returns true if this device is set as default
    pub fn is_default(&self) -> bool {
        self.contains(Self::DEFAULT)
    }

    /// Returns true if this device type supports both input and output.
    pub fn is_duplex(&self) -> bool {
        self.contains(Self::DUPLEX)
    }
}
