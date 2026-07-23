use coreaudio_sys::{AudioDeviceID, AudioObjectGetPropertyData, AudioObjectPropertyAddress};

pub(crate) fn get_device_property<T>(
    device_id: AudioDeviceID,
    address: AudioObjectPropertyAddress,
) -> Result<T, coreaudio::Error> {
    let mut data = std::mem::MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            data.as_mut_ptr() as *mut _,
        )
    };
    coreaudio::Error::from_os_status(status)?;
    Ok(unsafe { data.assume_init() })
}

#[expect(unused)]
pub(crate) fn set_device_property<T>(
    device_id: AudioDeviceID,
    address: AudioObjectPropertyAddress,
    data: &T,
) -> Result<(), coreaudio::Error> {
    let size = size_of::<T>() as u32;
    let status = unsafe {
        coreaudio_sys::AudioObjectSetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            size,
            data as *const T as *const _,
        )
    };
    coreaudio::Error::from_os_status(status)
}
