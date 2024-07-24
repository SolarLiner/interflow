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
}

fn get_inner_bitset_at<T: Bitset>(arr: &[T], index: usize) -> Option<(usize, usize)> {
    let mut total = 0;
    let bitset_index = arr.iter().position({
        move |b| {
            total += b.capacity();
            total > index
        }
    })?;
    let inner_index = index - total - arr[bitset_index].capacity();
    Some((bitset_index, inner_index))
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
