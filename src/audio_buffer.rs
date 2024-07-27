use std::collections::Bound;
use std::fmt;
use std::fmt::Formatter;
use std::ops::{AddAssign, RangeBounds};

use ndarray::{
    s, Array0, ArrayBase, ArrayView1, ArrayView2, ArrayViewMut1, ArrayViewMut2, AsArray, CowRepr,
    Data, DataMut, DataOwned, Ix1, Ix2, OwnedArcRepr, OwnedRepr, RawData, RawDataClone, ViewRepr,
};

pub type AudioBuffer<T> = AudioBufferBase<OwnedRepr<T>>;
pub type AudioRef<'a, T> = AudioBufferBase<ViewRepr<&'a T>>;
pub type AudioMut<'a, T> = AudioBufferBase<ViewRepr<&'a mut T>>;
pub type AudioShared<T> = AudioBufferBase<OwnedArcRepr<T>>;
pub type AudioCow<'a, T> = AudioBufferBase<CowRepr<'a, T>>;

type Storage<S> = ArrayBase<S, Ix2>;

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
    pub fn num_samples(&self) -> usize {
        self.storage.ncols()
    }

    pub fn num_channels(&self) -> usize {
        self.storage.nrows()
    }
}

impl<S: Data> AudioBufferBase<S> {
    pub fn as_ref(&self) -> AudioRef<S::Elem> {
        AudioRef {
            storage: self.storage.view(),
        }
    }

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

    pub fn get_channel(&self, channel: usize) -> ArrayView1<S::Elem> {
        self.storage.row(channel)
    }

    pub fn channels(&self) -> impl '_ + Iterator<Item = ArrayView1<S::Elem>> {
        self.storage.rows().into_iter()
    }

    pub fn get_frame(&self, sample: usize) -> ArrayView1<S::Elem> {
        self.storage.column(sample)
    }

    pub fn as_interleaved(&self) -> ArrayView2<S::Elem> {
        self.storage.t()
    }

    pub fn to_owned(&self) -> AudioBuffer<S::Elem>
    where
        S::Elem: Clone,
    {
        AudioBuffer {
            storage: self.storage.to_owned(),
        }
    }

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
        return true;
    }
}

impl<S: DataMut> AudioBufferBase<S> {
    pub fn as_mut(&mut self) -> AudioMut<S::Elem> {
        AudioMut {
            storage: self.storage.view_mut(),
        }
    }

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

    pub fn get_channel_mut(&mut self, channel: usize) -> ArrayViewMut1<S::Elem> {
        self.storage.row_mut(channel)
    }

    pub fn channels_mut(&mut self) -> impl '_ + Iterator<Item = ArrayViewMut1<S::Elem>> {
        self.storage.rows_mut().into_iter()
    }

    pub fn as_interleaved_mut(&mut self) -> ArrayViewMut2<S::Elem> {
        self.storage.view_mut().reversed_axes()
    }
}

impl<S: DataOwned> AudioBufferBase<S> {
    pub fn fill_with(
        channels: usize,
        buffer_size: usize,
        fill: impl Fn(usize, usize) -> S::Elem,
    ) -> Self {
        let storage = Storage::from_shape_fn((channels, buffer_size), |(ch, i)| fill(ch, i));
        Self { storage }
    }

    pub fn fill(channels: usize, buffer_size: usize, value: S::Elem) -> Self
    where
        S::Elem: Copy,
    {
        Self::fill_with(channels, buffer_size, |_, _| value)
    }

    pub fn defaulted(channels: usize, buffer_size: usize) -> Self
    where
        S::Elem: Default,
    {
        Self::fill_with(channels, buffer_size, |_, _| S::Elem::default())
    }
}

impl<'a, T: 'a> AudioRef<'a, T>
where
    ViewRepr<&'a T>: Sized,
{
    pub fn from_interleaved(data: &'a [T], channels: usize) -> Option<Self> {
        let buffer_size = data.len() / channels;
        let raw = ArrayView2::from_shape((buffer_size, channels), data).ok()?;
        let storage = raw.reversed_axes();
        Some(Self { storage })
    }
}

impl<'a, T: 'a> AudioMut<'a, T> {
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
    pub fn set_frame<'a>(&mut self, sample: usize, data: impl AsArray<'a, S::Elem, Ix1>)
    where
        S::Elem: 'a,
    {
        let column = self.storage.column_mut(sample);
        data.into().assign_to(column);
    }

    pub fn set_mono(&mut self, i: usize, value: S::Elem) {
        Array0::from_elem([], value)
            .broadcast((self.num_channels(),))
            .unwrap()
            .assign_to(self.storage.column_mut(i))
    }
}

pub trait Sample: Copy {
    type Float: Copy;
    const ZERO: Self;

    fn from_float(f: Self::Float) -> Self;

    fn rms(it: impl Iterator<Item = Self>) -> Self::Float;

    fn into_float(self) -> Self::Float;

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
    pub fn zeroed(channels: usize, buffer_size: usize) -> Self {
        Self::fill(channels, buffer_size, T::ZERO)
    }
}

impl<'a, S: Data> AudioBufferBase<S>
where
    S::Elem: Sample,
{
    pub fn rms(&self) -> <S::Elem as Sample>::Float {
        S::Elem::rms(self.storage.iter().copied())
    }

    pub fn channel_rms(&self, channel: usize) -> <S::Elem as Sample>::Float {
        S::Elem::rms(self.storage.column(channel).iter().copied())
    }
}

impl<'a, T: Sample> AudioMut<'a, T> {
    pub fn change_amplitude(&mut self, amplitude: T::Float) {
        for s in self.storage.iter_mut() {
            s.change_amplitude(amplitude);
        }
    }

    pub fn mix(&mut self, other: AudioRef<T>, other_amplitude: T::Float)
    where
        T: AddAssign<T>,
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
