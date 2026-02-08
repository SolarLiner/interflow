use std::num::NonZeroUsize;
use std::ops;
use zerocopy::FromZeros;

/// Audio buffer type. Data is stored in a contiguous array, in non-interleaved format.
#[derive(Clone)]
pub struct AudioBuffer<T> {
    data: Box<[T]>,
    frames: NonZeroUsize,
}

impl<T> ops::Index<usize> for AudioBuffer<T> {
    type Output = [T];
    fn index(&self, index: usize) -> &Self::Output {
        self.channel(index)
    }
}

impl<T> ops::IndexMut<usize> for AudioBuffer<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index * self.frames.get()..(index + 1) * self.frames.get()]
    }
}

impl<T> ops::Index<(usize, usize)> for AudioBuffer<T> {
    type Output = T;
    fn index(&self, (index, channel): (usize, usize)) -> &Self::Output {
        &self.data[channel * self.frames.get() + index]
    }
}

impl<T> ops::IndexMut<(usize, usize)> for AudioBuffer<T> {
    fn index_mut(&mut self, (index, channel): (usize, usize)) -> &mut Self::Output {
        &mut self.data[channel * self.frames.get() + index]
    }
}

impl<T: Copy> AudioBuffer<T>
where
    [T]: FromZeros,
{
    /// Creates a new buffer with the given number of frames and channels. The audio buffer will be zeroed out.
    pub fn zeroed(frames: NonZeroUsize, channels: NonZeroUsize) -> Self {
        let len = frames.get() * channels.get();
        AudioBuffer {
            data: <[T] as FromZeros>::new_box_zeroed_with_elems(len).unwrap(),
            frames,
        }
    }

    pub fn resize_channels(&mut self, channels: NonZeroUsize) {
        let mut data =
            <[T] as FromZeros>::new_box_zeroed_with_elems(channels.get() * self.frames.get())
                .unwrap();
        for channel in 0..channels.get() {
            let old_channel = channel % self.channels();
            let old_data =
                &self.data[old_channel * self.frames.get()..(old_channel + 1) * self.frames.get()];
            data[channel * self.frames.get()..(channel + 1) * self.frames.get()]
                .copy_from_slice(old_data);
        }
        self.data = data;
    }

    pub fn resize_frames(&mut self, frames: NonZeroUsize) {
        let mut data =
            <[T] as FromZeros>::new_box_zeroed_with_elems(frames.get() * self.channels()).unwrap();
        let min_frames = self.frames.min(frames).get();
        data[..min_frames * self.channels()]
            .copy_from_slice(&self.data[..min_frames * self.channels()]);
        self.frames = frames;
        self.data = data;
    }

    pub fn copy_to_interleaved(&self, out: &mut [T]) {
        debug_assert!(out.len() >= self.channels() * self.frames());
        for (i, sample) in out.iter_mut().enumerate() {
            let frame = i / self.channels();
            let channel = i % self.channels();
            let i = channel * self.frames.get() + frame;
            *sample = self.data[i];
        }
    }

    pub fn copy_from_interleaved(&mut self, data: &[T]) {
        debug_assert!(data.len() <= self.channels() * self.frames());
        for (i, sample) in data.iter().enumerate() {
            let frame = i / self.channels();
            let channel = i % self.channels();
            let i = channel * self.frames.get() + frame;
            self.data[i] = *sample;
        }
    }

    pub fn as_ref(&self) -> AudioRef<'_, T> {
        AudioRef {
            buffer: self,
            frame_slice: (0, self.frames.get()),
        }
    }

    pub fn as_mut(&mut self) -> AudioMut<'_, T> {
        let end = self.frames.get();
        AudioMut {
            buffer: self,
            frame_slice: (0, end),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FromDataError {
    #[error("Empty buffer")]
    Empty,
    #[error("Invalid number of channels: {channels} for buffer length {len} (len % channels) == {}", len % channels)]
    InvalidChannelCount { len: usize, channels: usize },
    #[error("Invalid number of frames: {frames} for buffer length {len} (len % frames) == {}", len % frames)]
    InvalidFrameCount { len: usize, frames: usize },
}

impl<T> AudioBuffer<T> {
    pub fn from_fn(
        channels: NonZeroUsize,
        frames: NonZeroUsize,
        f: impl Fn(usize, usize) -> T,
    ) -> Self {
        let mut data = Vec::with_capacity(channels.get() * frames.get());
        for channel in 0..channels.get() {
            for frame in 0..frames.get() {
                data.push(f(channel, frame));
            }
        }
        AudioBuffer {
            data: data.into_boxed_slice(),
            frames,
        }
    }

    pub fn from_data_channels(
        data: Box<[T]>,
        channels: NonZeroUsize,
    ) -> Result<Self, FromDataError> {
        if data.is_empty() {
            return Err(FromDataError::Empty);
        }
        if data.len() % channels.get() != 0 {
            return Err(FromDataError::InvalidChannelCount {
                len: data.len(),
                channels: channels.get(),
            });
        }

        let frames = NonZeroUsize::new(data.len() / channels.get()).unwrap();
        Ok(AudioBuffer { data, frames })
    }

    pub fn from_data_frames(data: Box<[T]>, frames: NonZeroUsize) -> Result<Self, FromDataError> {
        if data.is_empty() {
            return Err(FromDataError::Empty);
        }
        if data.len() % frames.get() != 0 {
            return Err(FromDataError::InvalidFrameCount {
                len: data.len(),
                frames: frames.get(),
            });
        }

        Ok(AudioBuffer { data, frames })
    }

    pub fn frames(&self) -> usize {
        self.frames.get()
    }

    pub fn channels(&self) -> usize {
        self.data.len() / self.frames.get()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    //noinspection ALL
    #[duplicate::duplicate_item(
    name        reference(type) out;
    [frame]     [&'_ type]      [FrameRef::<'_, T>];
    [frame_mut] [&'_ mut type]  [FrameMut::<'_, T>];
    )]
    pub fn name(self: reference([Self]), frame: usize) -> out {
        debug_assert!(frame < self.frames());
        out {
            buffer: self,
            frame,
        }
    }

    #[duplicate::duplicate_item(
    name   reference(type);
    [channel]     [& type];
    [channel_mut] [&mut type];
    )]
    pub fn name(self: reference([Self]), channel: usize) -> reference([[T]]) {
        debug_assert!(channel < self.channels());
        reference([self.data[channel * self.frames.get()..(channel + 1) * self.frames.get()]])
    }

    // noinspection ALL
    #[duplicate::duplicate_item(
    name   reference(type)      out;
    [slice]     [& type]        [AudioRef::<'_, T>];
    [slice_mut] [&mut type]     [AudioMut::<'_, T>];
    )]
    pub fn name(self: reference([Self]), index: impl ops::RangeBounds<usize>) -> out {
        let begin = match index.start_bound() {
            ops::Bound::Included(i) => *i,
            ops::Bound::Excluded(i) => *i + 1,
            ops::Bound::Unbounded => 0,
        };
        let end = match index.end_bound() {
            ops::Bound::Included(i) => *i - 1,
            ops::Bound::Excluded(i) => *i,
            ops::Bound::Unbounded => self.frames(),
        };
        debug_assert!(begin <= end);
        debug_assert!(end <= self.frames());
        out {
            buffer: self,
            frame_slice: (begin, end + 1),
        }
    }

    pub fn chunks(&self, size: usize) -> impl Iterator<Item = AudioRef<'_, T>> {
        (0..self.frames())
            .step_by(size)
            .map(move |frame| self.slice(frame..(frame + size).min(self.frames())))
    }

    pub fn chunks_exact(&self, size: usize) -> impl Iterator<Item = AudioRef<'_, T>> {
        (0..self.frames()).step_by(size).filter_map(move |frame| {
            let end = frame + size;
            if end > self.frames() {
                return None;
            }
            Some(self.slice(frame..end))
        })
    }

    pub fn windows(&self, size: usize) -> impl Iterator<Item = AudioRef<'_, T>> {
        (0..self.frames() - size).map(move |frame| self.slice(frame..(frame + size)))
    }

    pub fn iter_frames(&self) -> impl Iterator<Item = FrameRef<'_, T>> {
        (0..self.frames()).map(move |frame| self.frame(frame))
    }

    pub fn iter_frames_mut(&mut self) -> impl Iterator<Item = FrameMut<'_, T>> {
        IterFramesMut {
            buffer: self,
            frame: 0,
        }
    }

    pub fn iter_channels(&self) -> impl Iterator<Item = &[T]> {
        self.data.chunks(self.frames.get())
    }

    pub fn iter_channels_mut(&mut self) -> impl Iterator<Item = &mut [T]> {
        self.data.chunks_mut(self.frames.get())
    }

    pub fn get_channels<const N: usize>(&self, indices: [usize; N]) -> [&[T]; N] {
        indices.map(|i| self.channel(i))
    }

    pub fn get_channels_mut<const N: usize>(&mut self, indices: [usize; N]) -> [&mut [T]; N] {
        self.data.get_disjoint_mut(indices.map(|i| i * self.frames.get()..(i + 1) * self.frames.get())).unwrap()
    }
}

#[duplicate::duplicate_item(
name        reference(lifetime, type)       derive;
[FrameRef]  [&'lifetime type]               [derive(Clone, Copy)];
[FrameMut]  [&'lifetime mut type]           [derive()]
)]
#[derive]
pub struct name<'a, T> {
    buffer: reference([a], [AudioBuffer<T>]),
    frame: usize,
}

#[duplicate::duplicate_item(
name;
[FrameRef];
[FrameMut];
)]
impl<T: Copy> name<'_, T> {
    pub fn get(&self, channel: usize) -> T {
        debug_assert!(channel < self.buffer.channels());
        self.buffer[channel][self.frame]
    }

    pub fn get_frame(&self, out: &mut [T]) {
        debug_assert!(out.len() >= self.buffer.channels());
        for (channel, value) in out.iter_mut().enumerate() {
            *value = self.get(channel);
        }
    }
}

impl<T: Copy> FrameMut<'_, T> {
    pub fn set(&mut self, channel: usize, value: T) {
        debug_assert!(channel < self.buffer.channels());
        self.buffer[channel][self.frame] = value;
    }

    pub fn set_frame(&mut self, data: &[T]) {
        debug_assert!(data.len() >= self.buffer.channels());
        for (channel, value) in data.iter().enumerate() {
            self.set(channel, *value);
        }
    }
}

struct IterFramesMut<'a, T> {
    buffer: &'a mut AudioBuffer<T>,
    frame: usize,
}

impl<'a, T> Iterator for IterFramesMut<'a, T> {
    type Item = FrameMut<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.frame < self.buffer.frames() {
            let frame = self.frame;
            self.frame += 1;
            // SAFETY:
            // Lifetime of the frame is actually 'a, but the compiler cannot see that
            unsafe {
                let buffer_ptr = self.buffer as *mut AudioBuffer<T>;
                Some((*buffer_ptr).frame_mut(frame))
            }
        } else {
            None
        }
    }
}

#[duplicate::duplicate_item(
name        reference(lifetime, type)       derive;
[AudioRef]  [&'lifetime type]               [derive(Clone, Copy)];
[AudioMut]  [&'lifetime mut type]           [derive()]
)]
#[derive]
pub struct name<'a, T> {
    buffer: reference([a], [AudioBuffer<T>]),
    frame_slice: (usize, usize),
}

#[duplicate::duplicate_item(
name;
[AudioRef];
[AudioMut];
)]
impl<T> ops::Index<usize> for name<'_, T> {
    type Output = [T];

    fn index(&self, index: usize) -> &Self::Output {
        &self.buffer[self.frame_slice.0 + index]
    }
}

impl<T> ops::IndexMut<usize> for AudioMut<'_, T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.buffer[self.frame_slice.0 + index]
    }
}

#[duplicate::duplicate_item(
name;
[AudioRef];
[AudioMut];
)]
impl<T> name<'_, T> {
    pub fn frame(&self, index: usize) -> FrameRef<'_, T> {
        self.buffer.frame(self.frame_slice.0 + index)
    }

    pub fn channel(&self, channel: usize) -> &[T] {
        let slice = self.buffer.channel(channel);
        &slice[self.frame_slice.0..self.frame_slice.1]
    }
}

impl<T> AudioMut<'_, T> {
    pub fn frame_mut(&mut self, index: usize) -> FrameMut<'_, T> {
        let frame = index + self.frame_slice.0;
        debug_assert!(frame < self.frame_slice.1);
        FrameMut {
            buffer: self.buffer,
            frame,
        }
    }

    pub fn channel_mut(&mut self, channel: usize) -> &mut [T] {
        let slice = self.buffer.channel_mut(channel);
        &mut slice[self.frame_slice.0..self.frame_slice.1]
    }
}
