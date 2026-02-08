use std::any::TypeId;
use std::marker::PhantomData;
use std::mem::MaybeUninit;

/// A fully type-erased pointer, that can work with both thin and fat pointers.
/// Copied from <https://users.rust-lang.org/t/type-erasing-pointers-to-t-sized/96984>.
#[derive(Copy, Clone)]
struct ErasedPtr {
    value: MaybeUninit<[usize; 2]>,
}

impl ErasedPtr {
    /// Erase `ptr`.
    fn new<T: ?Sized>(ptr: *const T) -> Self {
        let mut res = ErasedPtr {
            value: MaybeUninit::zeroed(),
        };

        let len = size_of::<*const T>();
        assert!(len <= size_of::<[usize; 2]>());

        let ptr_val = (&ptr) as *const *const T as *const u8;
        let target = res.value.as_mut_ptr() as *mut u8;
        // SAFETY: The target is valid for at least `len` bytes, and has no
        // requirements on the value.
        unsafe {
            core::ptr::copy_nonoverlapping(ptr_val, target, len);
        }

        res
    }

    /// Convert the type erased pointer back into a pointer.
    ///
    /// # Safety
    ///
    /// The type `T` must be the same type as the one used with `new`.
    unsafe fn as_ptr<T: ?Sized>(&self) -> *const T {
        // SAFETY: The constructor ensures that the first `size_of::<T>()`
        // bytes of `&self.value` are a valid `*const T` pointer.
        unsafe {
            core::mem::transmute_copy(&self.value)
        }
    }
}

/// Type that can dynamically retrieve a type registered by an [`ExtensionProvider`] object.
/// Types are queried on-demand, every time an extension is requested.
///
/// Consumers of [`ExtensionProvider`] should instead use [`ExtensionExt::lookup`].
pub struct Selector<'a> {
    __lifetime: PhantomData<&'a ()>,
    target: TypeId,
    found: Option<ErasedPtr>,
}

impl<'a> Selector<'a> {
    pub(crate) const fn new<T: 'static + ?Sized>() -> Self {
        Self {
            __lifetime: PhantomData,
            target: TypeId::of::<T>(),
            found: None,
        }
    }

    pub fn register<T: 'static + ?Sized>(&mut self, value: &T) -> &mut Self {
        if self.target == TypeId::of::<T>() {
            self.found = Some(ErasedPtr::new(value));
        }
        self
    }

    pub(crate) fn finish<I: 'static + ?Sized>(self) -> Option<&'a I> {
        assert_eq!(self.target, TypeId::of::<I>());
        Some(unsafe { &*self.found?.as_ptr() })
    }
}

/// Trait for types that have optional data available on-demand.
pub trait ExtensionProvider: 'static {
    /// Register on-demand types. Note that the implementation details around registering extensions mean that this
    /// function will be called for every request. Runtime checks are expected, but this function should remain as fast
    /// as possible.
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a>;
}

const _EXTENSION_TRAIT_ASSERTS: () = {
    const fn typeable<T: ?Sized>() {}
    typeable::<dyn ExtensionProvider>();
};

/// Additional extensions for [`ExtensionProvider`] objects.
pub trait ExtensionProviderExt: ExtensionProvider {
    /// Look up [`T`] from the extension if it is registered.
    fn lookup<T: 'static + ?Sized>(&self) -> Option<&T> {
        let mut selector = Selector::new::<T>();
        { self.register(&mut selector); }
        selector.finish::<T>()
    }
}