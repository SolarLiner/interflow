use crate::device::DeviceType;
use crate::device::{AudioDevice, AudioDuplexDevice};
use std::borrow::Cow;

/// Audio drivers provide access to the inputs and outputs of physical devices.
/// Several drivers might provide the same accesses, some sharing it with other applications,
/// while others work in exclusive mode.
pub trait AudioDriver {
    /// Type of errors that can happen when using this audio driver.
    type Error: std::error::Error;
    /// Type of audio devices this driver provides.
    type Device: AudioDevice;

    /// Driver display name.
    const DISPLAY_NAME: &'static str;

    /// Runtime version of the audio driver. If there is a difference between "client" and
    /// "server" versions, then this should reflect the server version.
    fn version(&self) -> Result<Cow<str>, Self::Error>;

    /// Default device of the given type. This is most often tied to the audio settings at the
    /// operating system level.
    fn default_device(&self, device_type: DeviceType) -> Result<Option<Self::Device>, Self::Error>;

    /// List all devices available through this audio driver.
    fn list_devices(&self) -> Result<impl IntoIterator<Item = Self::Device>, Self::Error>;
}

/// Audio drivers that support duplex (simultaneous input/output) devices.
/// This extends the basic [`AudioDriver`] trait with duplex-specific functionality.
pub trait AudioDuplexDriver: AudioDriver {
    /// Type of duplex audio devices this driver provides.
    type DuplexDevice: AudioDuplexDevice;

    /// Returns the default duplex device for this driver, if one exists.
    /// This is typically determined by the system's audio settings.
    fn default_duplex_device(&self) -> Result<Option<Self::DuplexDevice>, Self::Error>;

    /// Lists all available duplex devices supported by this driver.
    /// Returns an iterator over the duplex devices.
    fn list_duplex_devices(
        &self,
    ) -> Result<impl IntoIterator<Item = Self::DuplexDevice>, Self::Error>;

    /// Creates a duplex device from separate input and output devices.
    /// This allows combining independent input and output devices into a single duplex device.
    fn device_from_input_output(
        &self,
        input: Self::Device,
        output: Self::Device,
    ) -> Result<Self::DuplexDevice, Self::Error>;
}
