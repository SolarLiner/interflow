use std::any::TypeId;
use std::marker::PhantomData;
use std::mem::MaybeUninit;

/// A fully type-erased pointer that can work with both thin and fat pointers.
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

    /// Erase `ptr`.
    fn new_mut<T: ?Sized>(ptr: *mut T) -> Self {
        let mut res = ErasedPtr {
            value: MaybeUninit::zeroed(),
        };

        let len = size_of::<*const T>();
        assert!(len <= size_of::<[usize; 2]>());

        let ptr_val = (&ptr) as *const *mut T as *mut u8;
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
    /// The type `T` must be the same type as the one used with `new` or `new_mut`.
    unsafe fn as_ptr<T: ?Sized>(&self) -> *const T {
        // SAFETY: The constructor ensures that the first `size_of::<T>()`
        // bytes of `&self.value` are a valid `*const T` pointer.
        unsafe { core::mem::transmute_copy(&self.value) }
    }

    /// Convert the type erased pointer back into a pointer.
    ///
    /// # Safety
    ///
    /// This [`ErasedPtr`] must have been constructed from [`Selector::new_mut`], using the same `T` type used in
    /// this function.
    unsafe fn as_mut_ptr<T: ?Sized>(&self) -> *mut T {
        // SAFETY: The constructor ensures that the first `size_of::<T>()`
        // bytes of `&self.value` are a valid `*const T` pointer.
        // The caller ensures that the original pointer was mutable.
        unsafe { core::mem::transmute_copy(&self.value) }
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
    found_mut: Option<ErasedPtr>,
}

impl<'a> Selector<'a> {
    pub(crate) const fn new<T: 'static + ?Sized>() -> Self {
        Self {
            __lifetime: PhantomData,
            target: TypeId::of::<T>(),
            found: None,
            found_mut: None,
        }
    }

    pub fn register<T: 'static + ?Sized>(&mut self, value: &T) -> &mut Self {
        if self.target == TypeId::of::<T>() {
            self.found = Some(ErasedPtr::new(value));
        }
        self
    }

    pub fn register_mut<T: 'static + ?Sized>(&mut self, value: &mut T) -> &mut Self {
        if self.target == TypeId::of::<T>() {
            self.found_mut = Some(ErasedPtr::new_mut(value));
        }
        self
    }

    pub(crate) fn finish<I: 'static + ?Sized>(self) -> Option<&'a I> {
        assert_eq!(self.target, TypeId::of::<I>());
        let target = self.found.or(self.found_mut)?;
        Some(unsafe { &*target.as_ptr() })
    }

    pub(crate) fn finish_mut<I: 'static + ?Sized>(self) -> Option<&'a mut I> {
        assert_eq!(self.target, TypeId::of::<I>());
        Some(unsafe { &mut *self.found_mut?.as_mut_ptr() })
    }
}

/// Trait for types that have optional data available on-demand.
pub trait ExtensionProvider: 'static {
    /// Register on-demand types. Note that the implementation details around registering extensions mean that this
    /// function will be called for every request. Runtime checks are expected, but this function should remain as fast
    /// as possible.
    fn register<'a, 'sel>(&'a self, selector: &'sel mut Selector<'a>) -> &'sel mut Selector<'a>;
    fn register_mut<'a, 'sel>(
        &'a mut self,
        selector: &'sel mut Selector<'a>,
    ) -> &'sel mut Selector<'a> {
        self.register(selector)
    }
}

const _EXTENSION_TRAIT_ASSERTS: () = {
    const fn typeable<T: ?Sized>() {}
    typeable::<dyn ExtensionProvider>();
};

pub trait ExtensionProviderExt: ExtensionProvider {
    /// Look up [`T`] from the extension if it is registered. Returns an immutable reference.
    fn lookup<T: 'static + ?Sized>(&self) -> Option<&T>;

    /// Look up [`T`] from the extension if it is registered. Returns as mutable reference.
    fn lookup_mut<T: 'static + ?Sized>(&mut self) -> Option<&mut T>;
}

impl<E: ?Sized + ExtensionProvider> ExtensionProviderExt for E {
    fn lookup<T: 'static + ?Sized>(&self) -> Option<&T> {
        let mut selector = Selector::new::<T>();
        selector.register::<E>(self);
        self.register(&mut selector);
        selector.finish::<T>()
    }

    fn lookup_mut<T: 'static + ?Sized>(&mut self) -> Option<&mut T> {
        let mut selector = Selector::new::<T>();
        selector.register_mut::<E>(self);
        self.register_mut(&mut selector);
        selector.finish_mut::<T>()
    }
}
