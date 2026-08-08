use crate::proxies::DynPlatform;
use scattered_collect::{gather, ScatteredSlice};

pub type PlatformConstructor = fn() -> Option<DynPlatform>;

/// Registration type for platforms to be used in `interflow::default_platform`.
pub struct Registration {
    pub constructor: PlatformConstructor,
    pub priority: i32,
}

/// Registrar. Do not use directly, instead call [`get_registry`] which returns a sorted list of constructors.
#[gather]
pub static REGISTRAR: ScatteredSlice<Registration>;

/// Returns a sorted list of constructors to try in sequence.
pub fn get_registry() -> impl Iterator<Item = PlatformConstructor> {
    let mut collected = Vec::from_iter(REGISTRAR.iter());
    collected.sort_by_key(|reg| -reg.priority);
    collected.into_iter().map(|reg| reg.constructor)
}

pub fn construct_platform() -> Option<DynPlatform> {
    for registration in get_registry() {
        if let Some(platform) = registration() {
            return Some(platform);
        }
    }
    None
}

pub const DEFAULT_PLATFORM_PRIORITY: i32 = 100;
