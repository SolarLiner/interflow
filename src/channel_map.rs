use core::panic;

pub trait Bitset: Sized {
    fn capacity(&self) -> usize;

    fn get_index(&self, index: usize) -> bool;

    fn set_index(&mut self, index: usize, value: bool);

    fn indices(&self) -> impl IntoIterator<Item = usize> {
        (0..self.capacity()).filter_map(|i| self.get_index(i).then_some(i))
    }

    fn with_index(&mut self, index: usize, value: bool) -> &mut Self {
        self.set_index(index, value);
        self
    }

    fn with_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        for ix in indices {
            self.set_index(ix, true);
        }
        self
    }

    fn count(&self) -> usize {
        self.indices().into_iter().count()
    }
}

#[duplicate::duplicate_item(
    ty;
    [u8];
    [u16];
    [u32];
    [u64];
    [u128];
)]
impl Bitset for ty {
    fn capacity(&self) -> usize {
        ty::BITS as usize
    }

    fn get_index(&self, index: usize) -> bool {
        let mask = 1 << index;
        self & mask > 0
    }

    fn set_index(&mut self, index: usize, value: bool) {
        let mask = 1 << index;
        if value {
            *self |= mask;
        } else {
            *self &= !mask;
        }
    }

    fn count(&self) -> usize {
        self.count_ones() as _
    }
}

fn get_inner_bitset_at<T: Bitset>(arr: &[T], mut index: usize) -> Option<(usize, usize)> {
    arr.iter().enumerate().find_map({
        let mut acc = 0;
        move |(i, b)| {
            return match index.checked_sub(b.capacity()) {
                None => Some((i, index)),
                Some(v) => {
                    index = v;
                    acc += b.capacity();
                    None
                }
            };
        }
    })
}

impl<'a, T: Bitset> Bitset for &'a mut [T] {
    fn capacity(&self) -> usize {
        self.iter().map(|b| b.capacity()).sum()
    }

    fn get_index(&self, index: usize) -> bool {
        let Some((bitset_index, inner_index)) = get_inner_bitset_at(self, index) else {
            return false;
        };
        self[bitset_index].get_index(inner_index)
    }

    fn set_index(&mut self, index: usize, value: bool) {
        let Some((bitset_index, inner_index)) = get_inner_bitset_at(self, index) else {
            panic!("Index {index} outside of range {}", self.capacity());
        };
        self[bitset_index].set_index(inner_index, value);
    }
}

pub type ChannelMap32 = u32;
pub type ChannelMap64 = u64;
pub type ChannelMap128 = u128;

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::hash::RandomState;

    use crate::channel_map::Bitset;

    #[test]
    fn test_getset_index() {
        let mut bitset = 0u8;
        bitset.set_index(0, true);
        bitset.set_index(2, true);
        bitset.set_index(3, true);
        bitset.set_index(2, false);

        assert_eq!(0b1001, bitset);
        assert!(bitset.get_index(0));
        assert!(bitset.get_index(3));
        assert!(!bitset.get_index(2));
    }

    #[test]
    fn test_indices() {
        let bitset = 0b10010100u8;
        let result = HashSet::<_, RandomState>::from_iter(bitset.indices());
        assert_eq!(HashSet::from_iter([2, 4, 7]), result);
    }

    #[test]
    fn test_slice_getset() {
        let mut storage = [0; 3];
        let mut bitset: &mut [u32] = &mut storage;

        bitset.set_index(0, true);
        bitset.set_index(34, true);
        bitset.set_index(81, true);

        assert_eq!([0b1, 0b100, 1 << (81 - 64)], bitset);

        assert!(bitset.get_index(0));
        assert!(bitset.get_index(34));
        assert!(bitset.get_index(81));
    }

    #[test]
    fn test_slice_indices() {
        let mut storage = [0b100101u8, 1 << 6 | 1 << 4, 1];
        let mut bitrate: &mut [u8] = &mut storage;
        let result = HashSet::<_, RandomState>::from_iter(bitrate.indices());
        assert_eq!(HashSet::from_iter([0, 2, 5, 12, 14, 16]), result);
    }
}
