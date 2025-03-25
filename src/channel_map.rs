//! This module provides functionality for working with bitsets and channel mapping.
//!
//! A bitset is a data structure that efficiently stores a set of boolean values using bits.
//! Each bit represents a boolean state (true/false) for a specific index or channel.
//!
//! The module includes:
//! - Generic `Bitset` trait for types that can represent sets of boolean values
//! - `CreateBitset` trait for constructing bitsets from indices
//! - Implementations for standard unsigned integer types (u8, u16, u32, u64, u128)
//! - Slice-based implementation for working with arrays of bitsets
//! - Type aliases for common channel map sizes (32, 64, and 128 bits)
//!
//! # Example
//!
//! ```
//! use interflow::channel_map::Bitset;
//!
//! let mut map = 0u32;
//! map.set_index(0, true);
//! map.set_index(5, true);
//! assert!(map.get_index(0));
//! assert!(map.get_index(5));
//! assert!(!map.get_index(1));
//! ```

use core::panic;

/// Trait for types which can represent bitsets.
///
/// A bit set is a type which encodes a boolean value, functioning similarly in principle to a
/// `HashSet<usize>`.
pub trait Bitset: Sized {
    /// Return the capacity of this bitset, that is, how many indices can be used with this type.
    fn capacity(&self) -> usize;

    /// Get the value for a specific index. Implementations should panic when this value is out
    /// of range.
    fn get_index(&self, index: usize) -> bool;

    /// Sets the value for a specific index. Implementations should panic when this value is out
    /// of range.
    fn set_index(&mut self, index: usize, value: bool);

    /// Returns an iterator of indices for which the value has been set `true`.
    fn indices(&self) -> impl IntoIterator<Item = usize> {
        (0..self.capacity()).filter(|i| self.get_index(*i))
    }
    /// Count the number of `true` elements in this bit set.
    fn count(&self) -> usize {
        self.indices().into_iter().count()
    }

    /// Builder-like method for setting a value at a specific index.
    fn with_index(&mut self, index: usize, value: bool) -> &mut Self {
        self.set_index(index, value);
        self
    }
    /// Builder-like method for setting all provided indices to `.
    fn with_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        for ix in indices {
            self.set_index(ix, true);
        }
        self
    }
}

/// Trait for bitsets that can be created from indices
pub trait CreateBitset: Bitset {
    /// Create a [`Self`] from the given indices
    ///
    /// # Arguments
    ///
    /// - `indices`: [`IntoIterator`] implementation that returns [`usize`] values corresponding to the indices to
    ///    set in the bitset.
    fn from_indices(indices: impl IntoIterator<Item = usize>) -> Self;
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

#[duplicate::duplicate_item(
    ty;
    [u8];
    [u16];
    [u32];
    [u64];
    [u128];
)]
impl CreateBitset for ty {
    fn from_indices(indices: impl IntoIterator<Item = usize>) -> Self {
        indices
            .into_iter()
            .inspect(|x| assert!(*x < Self::BITS as usize, "Index out of range"))
            .fold(0, |acc, ix| acc | (1 << ix))
    }
}

fn get_inner_bitset_at<T: Bitset>(arr: &[T], mut index: usize) -> Option<(usize, usize)> {
    arr.iter().enumerate().find_map({
        move |(i, b)| match index.checked_sub(b.capacity()) {
            None => Some((i, index)),
            Some(v) => {
                index = v;
                None
            }
        }
    })
}

impl<T: Bitset> Bitset for &mut [T] {
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

/// Type alias for a bitset with a capacity of 32 slots.
pub type ChannelMap32 = u32;
/// Type alias for a bitset with a capacity of 64 slots.
pub type ChannelMap64 = u64;
/// Type alias for a bitset with a capacity of 128 slots.
pub type ChannelMap128 = u128;

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::hash::RandomState;

    use super::*;

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
    fn test_from_indices() {
        let bitset = u8::from_indices([0, 2, 3]);
        assert_eq!(0b1101, bitset);
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
        let mut storage = [0b100101u8, (1 << 6) | (1 << 4), 1];
        let bitrate: &mut [u8] = &mut storage;
        let result = HashSet::<_, RandomState>::from_iter(bitrate.indices());
        assert_eq!(HashSet::from_iter([0, 2, 5, 12, 14, 16]), result);
    }
}
