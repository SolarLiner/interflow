use bitflags::bitflags;

pub mod buffer;
#[cfg(feature = "collect")]
pub mod collect;
pub mod device;
pub mod platform;
pub mod proxies;
pub mod stream;
pub mod timing;
pub mod traits;

pub mod prelude {
    pub use super::DeviceType;
    pub use crate::buffer::{AudioBuffer, AudioMut, AudioRef};
    pub use crate::device::{self, Device as _};
    pub use crate::platform;
    pub use crate::proxies::{self, CreateStreamExt};
    pub use crate::stream::{
        self, AudioInput, AudioOutput, ChannelFlags, StreamHandle, StreamLatency, StreamProxy,
    };
    pub use crate::timing::{self, *};
    pub use crate::traits::{self, *};
}

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

/// Adds compile-time checks that the given trait is dyn-safe.
#[macro_export]
macro_rules! dyn_compatible {
    ($(<$($generic:ident),+>)? $trait_:ident $(<$($(:$generic_name:ident =)?$generic_type:ident),+>)?) => {
        const _: () = {
            #[expect(unused)]
            const fn typeable$(<$($generic),*>)?(_: &dyn $trait_ $(<$($($generic_name =)? $generic_type),*>)?) {}
        };
    };
}
