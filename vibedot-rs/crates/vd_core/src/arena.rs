//! A generational arena: the backbone of the scene tree.
//!
//! Godot identifies objects with 64-bit `ObjectID`s and validates them against a
//! global object database. In Rust we get the same use-after-free protection
//! without a global lock: every slot carries a *generation* counter, and a
//! [`Handle`] is only valid while its generation matches the slot's.
//!
//! Layout notes (low-level details that matter for cache behaviour):
//! * Slots live in one contiguous `Vec`, so iteration is a linear scan.
//! * Free slots form an intrusive singly-linked free-list (no extra allocation).
//! * Generations are `NonZeroU32`, which gives `Option<Handle<T>>` a niche:
//!   `size_of::<Option<Handle<T>>>() == size_of::<Handle<T>>() == 8`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::NonZeroU32;

/// A typed, `Copy`, 8-byte handle into an [`Arena`].
pub struct Handle<T> {
    index: u32,
    generation: NonZeroU32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Slot index (stable for the lifetime of the referenced value).
    #[inline]
    pub fn index(self) -> u32 {
        self.index
    }

    /// Generation of the slot when this handle was issued.
    #[inline]
    pub fn generation(self) -> u32 {
        self.generation.get()
    }

    /// Packs the handle into a single `u64` (useful for hashing / FFI / debug).
    #[inline]
    pub fn to_bits(self) -> u64 {
        (u64::from(self.generation.get()) << 32) | u64::from(self.index)
    }

    /// Inverse of [`Handle::to_bits`]. Returns `None` for a zero generation.
    #[inline]
    pub fn from_bits(bits: u64) -> Option<Self> {
        let generation = NonZeroU32::new((bits >> 32) as u32)?;
        Some(Self { index: bits as u32, generation, _marker: PhantomData })
    }
}

// Manual impls: derives would add unwanted `T: Trait` bounds.
impl<T> Clone for Handle<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Handle<T> {}
impl<T> PartialEq for Handle<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}
impl<T> Eq for Handle<T> {}
impl<T> Hash for Handle<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.to_bits());
    }
}
impl<T> PartialOrd for Handle<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Handle<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.to_bits().cmp(&other.to_bits())
    }
}
impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}v{}", self.index, self.generation)
    }
}

#[derive(Clone)]
enum Slot<T> {
    Occupied { generation: NonZeroU32, value: T },
    Free { generation: NonZeroU32, next_free: u32 },
}

const NIL: u32 = u32::MAX;

/// Generational arena with O(1) insert / remove / lookup.
#[derive(Clone)]
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free_head: u32,
    len: u32,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for Arena<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<T> Arena<T> {
    pub const fn new() -> Self {
        Self { slots: Vec::new(), free_head: NIL, len: 0 }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self { slots: Vec::with_capacity(cap), free_head: NIL, len: 0 }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of slots ever allocated (occupied + free).
    #[inline]
    pub fn capacity_slots(&self) -> usize {
        self.slots.len()
    }

    pub fn insert(&mut self, value: T) -> Handle<T> {
        self.len += 1;
        if self.free_head != NIL {
            let index = self.free_head;
            let slot = &mut self.slots[index as usize];
            let Slot::Free { generation, next_free } = *slot else {
                unreachable!("free-list points at an occupied slot");
            };
            self.free_head = next_free;
            *slot = Slot::Occupied { generation, value };
            Handle { index, generation, _marker: PhantomData }
        } else {
            let index = u32::try_from(self.slots.len()).expect("arena exceeded u32::MAX slots");
            assert!(index != NIL, "arena exceeded u32::MAX - 1 slots");
            let generation = NonZeroU32::MIN;
            self.slots.push(Slot::Occupied { generation, value });
            Handle { index, generation, _marker: PhantomData }
        }
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        match slot {
            Slot::Occupied { generation, .. } if *generation == handle.generation => {
                // Bump generation so stale handles are rejected. Wrap from MAX back
                // to 1 (skipping 0) - after 4 billion reuses of a single slot.
                let next_gen = NonZeroU32::new(generation.get().wrapping_add(1)).unwrap_or(NonZeroU32::MIN);
                let old = std::mem::replace(slot, Slot::Free { generation: next_gen, next_free: self.free_head });
                self.free_head = handle.index;
                self.len -= 1;
                match old {
                    Slot::Occupied { value, .. } => Some(value),
                    Slot::Free { .. } => unreachable!(),
                }
            }
            _ => None,
        }
    }

    #[inline]
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    #[inline]
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        match self.slots.get(handle.index as usize)? {
            Slot::Occupied { generation, value } if *generation == handle.generation => Some(value),
            _ => None,
        }
    }

    #[inline]
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        match self.slots.get_mut(handle.index as usize)? {
            Slot::Occupied { generation, value } if *generation == handle.generation => Some(value),
            _ => None,
        }
    }

    /// Borrow two distinct elements mutably at once (safe split borrow).
    pub fn get2_mut(&mut self, a: Handle<T>, b: Handle<T>) -> Option<(&mut T, &mut T)> {
        if a.index == b.index {
            return None;
        }
        let (lo, hi, swapped) = if a.index < b.index { (a, b, false) } else { (b, a, true) };
        let (left, right) = self.slots.split_at_mut(hi.index as usize);
        let lo_ref = match left.get_mut(lo.index as usize)? {
            Slot::Occupied { generation, value } if *generation == lo.generation => value,
            _ => return None,
        };
        let hi_ref = match right.first_mut()? {
            Slot::Occupied { generation, value } if *generation == hi.generation => value,
            _ => return None,
        };
        Some(if swapped { (hi_ref, lo_ref) } else { (lo_ref, hi_ref) })
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.free_head = NIL;
        self.len = 0;
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, slot)| match slot {
            Slot::Occupied { generation, value } => {
                Some((Handle { index: i as u32, generation: *generation, _marker: PhantomData }, value))
            }
            Slot::Free { .. } => None,
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Handle<T>, &mut T)> + '_ {
        self.slots.iter_mut().enumerate().filter_map(|(i, slot)| match slot {
            Slot::Occupied { generation, value } => {
                Some((Handle { index: i as u32, generation: *generation, _marker: PhantomData }, value))
            }
            Slot::Free { .. } => None,
        })
    }

    pub fn handles(&self) -> impl Iterator<Item = Handle<T>> + '_ {
        self.iter().map(|(h, _)| h)
    }
}

impl<T> std::ops::Index<Handle<T>> for Arena<T> {
    type Output = T;
    #[track_caller]
    fn index(&self, handle: Handle<T>) -> &T {
        self.get(handle).unwrap_or_else(|| panic!("stale or invalid handle {handle:?}"))
    }
}

impl<T> std::ops::IndexMut<Handle<T>> for Arena<T> {
    #[track_caller]
    fn index_mut(&mut self, handle: Handle<T>) -> &mut T {
        self.get_mut(handle).unwrap_or_else(|| panic!("stale or invalid handle {handle:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_handle_has_niche() {
        assert_eq!(size_of::<Handle<String>>(), 8);
        assert_eq!(size_of::<Option<Handle<String>>>(), 8);
    }

    #[test]
    fn insert_get_remove() {
        let mut a = Arena::new();
        let h1 = a.insert("one");
        let h2 = a.insert("two");
        assert_eq!(a[h1], "one");
        assert_eq!(a.len(), 2);
        assert_eq!(a.remove(h1), Some("one"));
        assert!(a.get(h1).is_none(), "stale handle must be rejected");
        let h3 = a.insert("three");
        assert_eq!(h3.index(), h1.index(), "slot is reused");
        assert_ne!(h3, h1, "but with a new generation");
        assert_eq!(a[h2], "two");
        assert_eq!(a.iter().count(), 2);
    }

    #[test]
    fn split_borrow() {
        let mut a = Arena::new();
        let x = a.insert(1);
        let y = a.insert(2);
        let (rx, ry) = a.get2_mut(y, x).unwrap();
        std::mem::swap(rx, ry);
        assert_eq!((a[x], a[y]), (2, 1));
        assert!(a.get2_mut(x, x).is_none());
    }

    #[test]
    fn bits_roundtrip() {
        let mut a = Arena::new();
        let h = a.insert(());
        assert_eq!(Handle::<()>::from_bits(h.to_bits()), Some(h));
        assert_eq!(Handle::<()>::from_bits(0), None);
    }
}
