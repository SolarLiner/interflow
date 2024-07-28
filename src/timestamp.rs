use std::ops;
use std::ops::AddAssign;
use std::time::Duration;

/// Timestamp value, which computes duration information from a provided samplerate and a running
/// sample counter.
/// 
/// You can update the timestamp by add-assigning sample counts to it:
/// 
/// ```rust
/// use std::time::Duration;
/// use interflow::timestamp::Timestamp;
/// let mut ts = Timestamp::new(48000.);
/// assert_eq!(ts.as_duration(), Duration::from_nanos(0));
/// ts += 48;
/// assert_eq!(ts.as_duration(), Duration::from_millis(1));
/// ```
/// 
/// Adding also works, returning a new timestamp:
///
/// ```rust
/// use std::time::Duration;
/// use interflow::timestamp::Timestamp;
/// let mut ts = Timestamp::new(48000.);
/// assert_eq!(ts.as_duration(), Duration::from_nanos(0));
/// let ts2 = ts + 48;
/// assert_eq!(ts.as_duration(), Duration::from_millis(0));
/// assert_eq!(ts2.as_duration(), Duration::from_millis(1));
/// ```
/// 
/// Similarly, you can compute sample offsets by adding a [`Duration`] to it:
/// 
/// ```rust
/// use std::time::Duration;
/// use interflow::timestamp::Timestamp;
/// let ts = Timestamp::from_count(48000., 48);
/// let ts_off = ts + Duration::from_millis(100);
/// assert_eq!(ts_off.as_duration(), Duration::from_millis(101));
/// assert_eq!(ts_off.counter, 448);
/// ```
/// 
/// Or simply construct a [`Timestamp`] from a specified duration:
/// 
/// ```rust
/// use std::time::Duration;
/// use interflow::timestamp::Timestamp;
/// let ts = Timestamp::from_duration(44100., Duration::from_micros(44_100));
/// assert_eq!(ts.counter, 44); // Note that the conversion is lossy, as only whole samples are 
///                             // stored in the timestamp.
/// ```
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Timestamp {
    /// Number of samples counted in this timestamp.
    pub counter: u64,
    /// Samplerate of the audio stream associated with the counter.
    pub samplerate: f64,
}

impl AddAssign<Duration> for Timestamp {
    fn add_assign(&mut self, rhs: Duration) {
        let samples = rhs.as_secs_f64() * self.samplerate;
        self.counter += samples as u64;
    }
}

impl AddAssign<u64> for Timestamp {
    fn add_assign(&mut self, rhs: u64) {
        self.counter += rhs;
    }
}

impl<T> ops::Add<T> for Timestamp
where
    Self: AddAssign<T>,
{
    type Output = Self;

    fn add(mut self, rhs: T) -> Self {
        self.add_assign(rhs);
        self
    }
}

impl Timestamp {
    /// Create a zeroed timestamp with the provided sample rate.
    pub fn new(samplerate: f64) -> Self {
        Self {
            counter: 0,
            samplerate,
        }
    }

    /// Create a timestamp from the given sample rate and existing sample count.
    pub fn from_count(samplerate: f64, counter: u64) -> Self {
        Self {
            samplerate,
            counter,
        }
    }

    /// Compute the sample offset that most closely matches the provided duration for the given 
    /// sample rate.
    pub fn from_duration(samplerate: f64, duration: Duration) -> Self {
        Self::from_seconds(samplerate, duration.as_secs_f64())
    }

    /// Compute the sample offset that most closely matches the provided duration in seconds for 
    /// the given sample rate.
    pub fn from_seconds(samplerate: f64, seconds: f64) -> Self {
        let samples = samplerate * seconds;
        Self {
            samplerate,
            counter: samples as _,
        }
    }

    /// Compute the duration represented by this [`Timestamp`].
    pub fn as_duration(&self) -> Duration {
        Duration::from_secs_f64(self.as_seconds())
    }

    /// Compute the number of seconds represented in this [`Timestamp`].
    pub fn as_seconds(&self) -> f64 {
        self.counter as f64 / self.samplerate
    }
}
