use std::ops;
use std::ops::AddAssign;
use std::time::Duration;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Timestamp {
    pub counter: u64,
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
    pub fn new(samplerate: f64) -> Self {
        Self {
            counter: 0,
            samplerate,
        }
    }

    pub fn from_count(samplerate: f64, counter: u64) -> Self {
        Self {
            samplerate,
            counter,
        }
    }

    pub fn from_duration(samplerate: f64, duration: Duration) -> Self {
        Self::from_seconds(samplerate, duration.as_secs_f64())
    }

    pub fn from_seconds(samplerate: f64, seconds: f64) -> Self {
        let samples = samplerate * seconds;
        Self {
            samplerate,
            counter: samples as _,
        }
    }

    pub fn as_duration(&self) -> Duration {
        Duration::from_secs_f64(self.as_seconds())
    }

    pub fn as_seconds(&self) -> f64 {
        self.counter as f64 / self.samplerate
    }
}
