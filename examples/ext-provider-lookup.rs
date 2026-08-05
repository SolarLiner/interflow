//! An example of using the [`ExtensionProvider`] trait through [`ExtensionProviderExt::lookup`] to dynamically
//! lookup types and traits registered by the backend types.
use interflow::prelude::*;

fn main() -> anyhow::Result<()> {
    let device = default_device(DeviceType::OUTPUT)?;

    println!("Selected device name: {:?}", device.name());

    // Assuming the above device originates from platform-independent code, we can query for the concrete backend
    // type.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if let Some(device) = device.lookup::<coreaudio::Device>() {
        use interflow_coreaudio::device::CoreAudioDeviceExt as _;

        let audio_unit = device.get_audio_unit()?;
        let stream_format = audio_unit.output_stream_format()?;
        println!(
            "CoreAudio output stream format for {:?}: {stream_format:#?}",
            device.name()
        );
    }

    // There are traits that can be implemented by several backends, they can also be queried for and used if they
    // are registered by the backend type.
    if let Some(ext) = device.lookup::<dyn device::DeviceState>() {
        println!("Device is connected: {}", ext.connected());
    }

    // A concrete example is listing configurations: not all platforms support this feature, and instead of
    // "(un)graceful degradation", we instead only implement and register this trait when it makes sense.
    if let Some(enumerator) = device.lookup::<dyn device::ConfigurationList>() {
        for config in enumerator.enumerate_configurations() {
            println!("\tConfiguration: {config:?}");
        }
    } else {
        println!("Device does not support listing configurations");
    }

    // Another useful extension allows iterating over named channels of a device.
    if let Some(enumerator) = device.lookup::<dyn device::NamedChannels>() {
        for channel in enumerator.enumerate_channels() {
            println!("\tChannel {:3}: {}", channel.index, channel.name);
        }
    } else {
        println!("Device does not have named channels");
    }
    Ok(())
}
