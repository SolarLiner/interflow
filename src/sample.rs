use duplicate::duplicate_item;

pub trait ConvertSample: Sized + Copy {
    const ZERO: Self;

    fn convert_to_f32(self) -> f32;
    fn convert_from_f32(v: f32) -> Self;

    fn convert_to_slice(output: &mut [f32], input: &[Self]) {
        assert!(output.len() >= input.len());
        for (out, sample) in output.iter_mut().zip(input) {
            *out = sample.convert_to_f32();
        }
    }

    fn convert_from_slice(output: &mut [Self], input: &[f32]) {
        assert!(output.len() >= input.len());
        for (out, &sample) in output.iter_mut().zip(input) {
            *out = Self::convert_from_f32(sample);
        }
    }
}

impl ConvertSample for f32 {
    const ZERO: Self = 0.0;

    #[inline]
    fn convert_to_f32(self) -> f32 {
        self
    }

    #[inline]
    fn convert_from_f32(v: f32) -> Self {
        v
    }
}

#[duplicate_item(
int;
[i8];
[i16];
[i32];
)]
impl ConvertSample for int {
    const ZERO: Self = 0;

    fn convert_to_f32(self) -> f32 {
        self as f32 / Self::MAX as f32
    }

    fn convert_from_f32(f: f32) -> Self {
        (f * Self::MAX as f32) as Self
    }
}

#[duplicate_item(
uint    zero;
[u8]    [128];
[u16]   [32768];
[u32]   [2147483648];
)]
impl ConvertSample for uint {
    const ZERO: Self = zero;

    fn convert_to_f32(self) -> f32 {
        2.0 * self as f32 / Self::MAX as f32 - 1.0
    }

    fn convert_from_f32(f: f32) -> Self {
        ((f + 1.0) * Self::MAX as f32 / 2.0) as Self
    }
}
