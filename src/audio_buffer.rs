use std::collections::Bound;
use std::fmt;
use std::fmt::Formatter;
use std::ops::{AddAssign, RangeBounds};

use ndarray::{
    s, Array0, ArrayBase, ArrayView1, ArrayView2, ArrayViewMut1, ArrayViewMut2, AsArray, CowRepr,
    Data, DataMut, DataOwned, Ix1, Ix2, OwnedArcRepr, OwnedRepr, RawData, RawDataClone, ViewRepr,
};

/// Owned audio buffer type.
pub type AudioBuffer<T> = AudioBufferBase<OwnedRepr<T>>;
/// Immutably referenced audio buffer type.
pub type AudioRef<'a, T> = AudioBufferBase<ViewRepr<&'a T>>;
/// Mutably referenced audio buffer type.
pub type AudioMut<'a, T> = AudioBufferBase<ViewRepr<&'a mut T>>;
/// Arc-backed shared audio buffer type.
pub type AudioShared<T> = AudioBufferBase<OwnedArcRepr<T>>;
/// Copy-on-write audio buffer type. Should not be used within audio callbacks, as the copy will
/// intrroduce allocations.
pub type AudioCow<'a, T> = AudioBufferBase<CowRepr<'a, T>>;

type Storage<S> = ArrayBase<S, Ix2>;

/// Audio buffer type, which backs all audio data interfacing with user code.
///
/// This type is made to make manipulation of audio data easier, and is agnostic in its storage
/// representation, meaning that it can work with both interleaved and non-interleaved data.
pub struct AudioBufferBase<S: RawData> {
    storage: Storage<S>,
}

impl<S: RawData> fmt::Debug for AudioBufferBase<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioBufferBase")
            .field(
                "storage",
                &format!(
                    "[{}x{} buffer of {}]",
                    self.storage.nrows(),
                    self.storage.ncols(),
                    std::any::type_name::<S::Elem>()
                ),
            )
            .finish_non_exhaustive()
    }
}

impl<S: RawDataClone> Clone for AudioBufferBase<S> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
        }
    }
}

impl<S: Copy + RawDataClone> Copy for AudioBufferBase<S> {}

impl<S: DataOwned> Default for AudioBufferBase<S> {
    fn default() -> Self {
        Self {
            storage: ArrayBase::from_shape_fn((0, 0), |(_, _)| unreachable!()),
        }
    }
}

impl<S: Data, S2: Data<Elem = S::Elem>> PartialEq<AudioBufferBase<S2>> for AudioBufferBase<S>
where
    S::Elem: PartialEq<S::Elem>,
{
    fn eq(&self, other: &AudioBufferBase<S2>) -> bool {
        self.storage.shape() == other.storage.shape()
            && self.storage.iter().eq(other.storage.iter())
    }

    fn ne(&self, other: &AudioBufferBase<S2>) -> bool {
        self.storage.shape() != other.storage.shape()
            || self.storage.iter().ne(other.storage.iter())
    }
}

impl<S: Data> Eq for AudioBufferBase<S>
where
    Self: PartialEq<Self>,
    S::Elem: Eq,
{
}

impl<S: RawData> AudioBufferBase<S> {
    /// Number of samples present in this buffer.
    pub fn num_samples(&self) -> usize {
        self.storage.ncols()
    }

    /// Number of channels present in this buffer.
    pub fn num_channels(&self) -> usize {
        self.storage.nrows()
    }
}

impl<S: Data> AudioBufferBase<S> {
    /// Return an immutable audio buffer view, sharing the data with this buffer.
    pub fn as_ref(&self) -> AudioRef<S::Elem> {
        AudioRef {
            storage: self.storage.view(),
        }
    }

    /// Slice the contents of this audio buffer, returning an immutable view of this buffer
    /// containing only the audio samples at indices within the provided range.
    pub fn slice(&self, range: impl RangeBounds<usize>) -> AudioRef<S::Elem> {
        let start = match range.start_bound() {
            Bound::Included(i) => *i,
            Bound::Excluded(i) => *i + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(i) => *i - 1,
            Bound::Excluded(i) => *i,
            Bound::Unbounded => self.num_samples(),
        };
        let storage = self.storage.slice(s![.., start..end]);
        AudioRef { storage }
    }

    /// Return an immutable view of a single channel. Panics when the requested channel does not
    /// exist.
    pub fn get_channel(&self, channel: usize) -> ArrayView1<S::Elem> {
        self.storage.row(channel)
    }

    /// Return an iterator of immutable views of the channels present in this audio buffer.
    pub fn channels(&self) -> impl '_ + Iterator<Item = ArrayView1<S::Elem>> {
        self.storage.rows().into_iter()
    }

    /// Get a single frame, that is all channels at the specified sample index. Panics when the
    /// sample is out of range.
    pub fn get_frame(&self, sample: usize) -> ArrayView1<S::Elem> {
        self.storage.column(sample)
    }

    /// Return an immutable interleaved 2-D array view, where samples are in rows and channels are
    /// in columns.
    pub fn as_interleaved(&self) -> ArrayView2<S::Elem> {
        self.storage.t()
    }

    /// Copies this audio buffer to another, giving you a unique owned buffer in the end.
    ///
    /// Not realtime-safe.
    pub fn to_owned(&self) -> AudioBuffer<S::Elem>
    where
        S::Elem: Clone,
    {
        AudioBuffer {
            storage: self.storage.to_owned(),
        }
    }

    /// Copies audio data in this buffer to the provided interleaved buffer. The `output` buffer
    /// must represent an interleaved buffer with the same number of channels and same number of
    /// samples.
    #[must_use]
    pub fn copy_into_interleaved(&self, output: &mut [S::Elem]) -> bool
    where
        S::Elem: Copy,
    {
        if output.len() != self.storage.len() {
            return false;
        }

        for (inp, out) in self.as_interleaved().iter().zip(output.iter_mut()) {
            *out = *inp;
        }
        true
    }
}

impl<S: DataMut> AudioBufferBase<S> {
    /// Return a mutable audio buffer view.
    pub fn as_mut(&mut self) -> AudioMut<S::Elem> {
        AudioMut {
            storage: self.storage.view_mut(),
        }
    }

    /// Slice the contents of this audio buffer, returning a mutable view of this buffer
    /// containing only the audio samples at indices within the provided range.
    pub fn slice_mut(&mut self, range: impl RangeBounds<usize>) -> AudioMut<S::Elem> {
        let start = match range.start_bound() {
            Bound::Included(i) => *i,
            Bound::Excluded(i) => *i + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(i) => *i - 1,
            Bound::Excluded(i) => *i,
            Bound::Unbounded => self.num_samples(),
        };
        let storage = self.storage.slice_mut(s![.., start..end]);
        AudioMut { storage }
    }

    /// Return a mutable view of a single channel. Panics when the requested channel does not
    /// exist.
    pub fn get_channel_mut(&mut self, channel: usize) -> ArrayViewMut1<S::Elem> {
        self.storage.row_mut(channel)
    }

    /// Return an iterator of mutable views of the channels present in this audio buffer.
    pub fn channels_mut(&mut self) -> impl '_ + Iterator<Item = ArrayViewMut1<S::Elem>> {
        self.storage.rows_mut().into_iter()
    }
/// Return a mutable interleaved 2-D array view, where samples are in rows and channels are in
    /// columns.
    pub fn as_interleaved_mut(&mut self) -> ArrayViewMut2<S::Elem> {
        self.storage.view_mut().reversed_axes()
    }
}

impl<S: DataOwned> AudioBufferBase<S> {
    /// Create a new audio buffer with the provided number of channels and sample size, filling
    /// it with the provided fill function.
    ///
    /// Not realtime-safe.
    pub fn fill_with(
        channels: usize,
        sample_size: usize,
        fill: impl Fn(usize, usize) -> S::Elem,
    ) -> Self {
        let storage = Storage::from_shape_fn((channels, sample_size), |(ch, i)| fill(ch, i));
        Self { storage }
    }

    /// Create a new audio buffer with the provided number of channels and sample size, filling
    /// it with the provided value.
    pub fn fill(channels: usize, sample_size: usize, value: S::Elem) -> Self
    where
        S::Elem: Copy,
    {
        Self::fill_with(channels, sample_size, |_, _| value)
    }

    /// Create a new audio buffer with the provided number of channels and sample size, filling
    /// it with the [`Default`] value of that type.
    pub fn defaulted(channels: usize, sample_size: usize) -> Self
    where
        S::Elem: Default,
    {
        Self::fill_with(channels, sample_size, |_, _| S::Elem::default())
    }
}

impl<'a, T: 'a> AudioRef<'a, T>
where
    ViewRepr<&'a T>: Sized,
{
    /// Create an audio buffer reference from interleaved data. This does *not* copy the data,
    /// but creates a view over it, so that it can be accessed as any other audio buffer.
    pub fn from_interleaved(data: &'a [T], channels: usize) -> Option<Self> {
        let buffer_size = data.len() / channels;
        let raw = ArrayView2::from_shape((buffer_size, channels), data).ok()?;
        let storage = raw.reversed_axes();
        Some(Self { storage })
    }
}

impl<'a, T: 'a> AudioMut<'a, T> {
    /// Create an audio buffer mutable reference from interleaved data. This does *not* copy the
    /// data, but creates a view over it, so that it can be accessed as any other audio buffer.
    ///
    /// Writes to the resulting buffer directly map to the provided slice, and asking an
    /// interleaved view out of the resulting buffer (with [`AudioBufferBase::as_interleaved`])
    /// means the same slice is returned. This makes for efficient copying between different
    /// interleaved buffers, even though a non-interleaved interface.
    pub fn from_interleaved_mut(data: &'a mut [T], channels: usize) -> Option<Self> {
        let buffer_size = data.len() / channels;
        let raw = ArrayViewMut2::from_shape((buffer_size, channels), data).ok()?;
        let storage = raw.reversed_axes();
        Some(Self { storage })
    }
}

impl<S: DataMut> AudioBufferBase<S>
where
    S::Elem: Clone,
{
    /// Sets audio data of a single frame, that is all channels at the specified sample index.
    /// Panics when the sample is out of range.
    pub fn set_frame<'a>(&mut self, sample: usize, data: impl AsArray<'a, S::Elem, Ix1>)
    where
        S::Elem: 'a,
    {
        let column = self.storage.column_mut(sample);
        data.into().assign_to(column);
    }

    /// Sets audio data of a single sample, copying the provided value to each channel at that
    /// sample index. Panics when the sample index is out of range.
    pub fn set_mono(&mut self, i: usize, value: S::Elem) {
        Array0::from_elem([], value)
            .broadcast((self.num_channels(),))
            .unwrap()
            .assign_to(self.storage.column_mut(i))
    }
}

/// Trait for sample types. Typical sample types can be `i32`, `f32`, etc. but more can be
/// implemented downstream.
pub trait Sample: Copy {
    /// Floating-point type which can fit all or a big majority of this type's values.
    /// This type is the type used in float conversions, as well as the type of the amplitude in
    /// buffer amplitude operations.
    type Float: Copy;
    /// Zero value for this sample. This is *not specifically* the numerical zero of the type,
    /// but the value for which the amplitude of the stream is zero. Unsigned types are an
    /// example for which the two are different.
    const ZERO: Self;

    /// Construct a sample of this type from the corresponding float signal value.
    fn from_float(f: Self::Float) -> Self;

    /// Compute the RMS value out of an iterator of this type.
    fn rms(it: impl Iterator<Item = Self>) -> Self::Float;

    /// Convert this value into its floating point equivalent.
    fn into_float(self) -> Self::Float;

    /// Change the "amplitude" of this value, ie. absolute values less than one will bring the
    /// value closer to [`Self::ZERO`], whereas absolute values above one will move the value
    /// further away.
    fn change_amplitude(&mut self, amp: Self::Float);
}

#[duplicate::duplicate_item(
    ty    fty;
    [i8]  [f32];
    [i16] [f32];
    [i32] [f32];
    [i64] [f64];
)]
impl Sample for ty {
    type Float = fty;
    const ZERO: Self = 0;

    fn from_float(f: Self::Float) -> Self {
        (f * ty::MAX as fty) as ty
    }
    fn rms(it: impl Iterator<Item = Self>) -> Self::Float {
        it.map(|t| t as fty).map(|f| f.powi(2)).sum::<fty>().sqrt()
    }

    fn into_float(self) -> Self::Float {
        self as fty / ty::MAX as fty
    }
    fn change_amplitude(&mut self, amp: Self::Float) {
        *self = ((*self as fty) * amp) as Self;
    }
}

#[duplicate::duplicate_item(
    ty      fty;
    [u8]    [f32];
    [u16]   [f32];
    [u32]   [f32];
    [u64]   [f64];
)]
impl Sample for ty {
    type Float = fty;
    const ZERO: Self = Self::MAX / 2;

    fn from_float(f: Self::Float) -> Self {
        ((f * 0.5 + 0.5) * Self::MAX as Self::Float) as Self
    }

    fn rms(it: impl Iterator<Item = Self>) -> Self::Float {
        it.map(Self::into_float)
            .map(|x| x.powi(2))
            .sum::<Self::Float>()
            .sqrt()
    }

    fn into_float(self) -> Self::Float {
        let t = self as Self::Float / Self::MAX as Self::Float;
        t * 2.0 - 1.0
    }

    fn change_amplitude(&mut self, amp: Self::Float) {
        let f = Self::into_float(*self) * amp;
        *self = Self::from_float(f)
    }
}

#[duplicate::duplicate_item(
    ty;
    [f32];
    [f64];
)]
impl Sample for ty {
    type Float = Self;
    const ZERO: Self = 0.0;

    fn from_float(f: Self::Float) -> Self {
        f
    }

    fn rms(it: impl Iterator<Item = Self>) -> Self::Float {
        it.map(|x| x.powi(2)).sum::<Self>().sqrt()
    }

    fn into_float(self) -> Self::Float {
        self
    }

    fn change_amplitude(&mut self, amp: Self::Float) {
        *self *= amp;
    }
}

impl<T: Sample> AudioBuffer<T> {
    /// Construct a zeroed buffer with the provided channels and sample size.
    ///
    /// Not realtime-safe.
    pub fn zeroed(channels: usize, sample_size: usize) -> Self {
        Self::fill(channels, sample_size, T::ZERO)
    }
}

impl<'a, S: Data> AudioBufferBase<S>
where
    S::Elem: Sample,
{
    /// Compute the RMS (Root Mean Square) value of this entire buffer, all channels considered
    /// equally. The result is given in terms of linear amplitude values, as a float determined by
    /// [`S::Float`].
    ///
    /// You can convert the result to decibels with the formula `20. * rms.log10()`.
    pub fn rms(&self) -> <S::Elem as Sample>::Float {
        S::Elem::rms(self.storage.iter().copied())
    }

    /// Compute the RMS (Root Mean Square) value of this entire buffer for a single channel. The
    /// result is given in terms of linear amplitude values, as a float determined by [`S::Float`].
    ///
    /// You can convert the result to decibels with the formula `20. * rms.log10()`.
    pub fn channel_rms(&self, channel: usize) -> <S::Elem as Sample>::Float {
        S::Elem::rms(self.storage.column(channel).iter().copied())
    }
}

impl<'a, S: DataMut<Elem: Sample>> AudioBufferBase<S> {
    /// Change the amplitude of this buffer by the provided amplitude.
    ///
    /// See [`Sample::change_amplitude`] for more details.
    pub fn change_amplitude(&mut self, amplitude: <S::Elem as Sample>::Float) {
        for s in self.storage.iter_mut() {
            s.change_amplitude(amplitude);
        }
    }

    /// Mix a buffer into this buffer at the specified amplitude. The audio will be mixed into
    /// this buffer as a result, and the other buffer's amplitude will be changed similarly to
    /// applying [`Self::change_amplitude`] first.
    pub fn mix(&mut self, other: AudioRef<S::Elem>, other_amplitude: <S::Elem as Sample>::Float)
    where
        S::Elem: AddAssign<S::Elem>,
    {
        for (mut ch_a, ch_b) in self.channels_mut().zip(other.channels()) {
            for (a, b) in ch_a.iter_mut().zip(ch_b) {
                let mut b = *b;
                b.change_amplitude(other_amplitude);
                *a += b;
            }
        }
    }
}
