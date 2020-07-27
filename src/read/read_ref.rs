use super::ReadGuard;
use crate::inner::Inner;
use one_way_slot_map::{SlotMapKey as Key, SlotMapKeyData};
use std::marker::PhantomData;
use std::mem::ManuallyDrop;

use super::user_friendly;

/// A live reference into the read half of an evmap.
///
/// As long as this lives, the map being read cannot change, and if a writer attempts to
/// call any write method, that call will block until this is dropped, so make
/// sure these are dropped as soon as possible
///
/// Since the map remains immutable while this lives, the methods on this type all give you
/// unguarded references to types contained in the map.
#[derive(Debug)]
pub struct MapReadRef<'rh, K, P, V>
where
    K: Key<P>,
{
    pub(super) guard: ReadGuard<'rh, Inner<ManuallyDrop<V>>>,
    pub(super) _phantom_k: PhantomData<K>,
    pub(super) _phantom_p: PhantomData<P>,
}

impl<'rh, K, P, V> MapReadRef<'rh, K, P, V>
where
    K: Key<P>,
{
    /// Returns the number of non-empty keys present in the map.
    pub fn len(&self) -> usize {
        self.guard.data.len()
    }

    /// Returns true if the map contains no elements.
    pub fn is_empty(&self) -> bool {
        self.guard.data.is_empty()
    }

    /// Get an iterator over all the items in the slot map
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.guard.data.values().map(user_friendly)
    }

    /// Get an iterator over all the keys and values in the slot map as long
    /// as you have a way to create the pointer value from the stored value
    pub fn iter<F>(
        &self,
        mut pointer_finder: F,
    ) -> impl Iterator<Item = (K, &V)>
    where
        F: FnMut(&V) -> P,
    {
        self.iter_raw().map(move |(key_data, v)| {
            (K::from(((&mut pointer_finder)(v), key_data)), v)
        })
    }

    /// Get an iterator over all the raw key data and values in the map
    pub fn iter_raw(&self) -> impl Iterator<Item = (SlotMapKeyData, &V)> {
        self.guard
            .data
            .iter_raw()
            .map(|(key_data, v)| (key_data, user_friendly(v)))
    }

    /// Returns a reference to the values corresponding to the key.
    ///
    /// The key may be any borrowed form of the map's key type, but `Hash` and `Eq` on the borrowed
    /// form *must* match those for the key type.
    ///
    /// Note that not all writes will be included with this read -- only those that have been
    /// refreshed by the writer. If no refresh has happened, or the map has been destroyed, this
    /// function returns `None`.
    pub fn get<'a>(&'a self, key: &'_ K) -> Option<&'a V> {
        self.guard.data.get_unbounded(key).map(user_friendly)
    }

    /// Returns true if the map contains any values for the specified key.
    ///
    /// The key may be any borrowed form of the map's key type, but `Hash` and `Eq` on the borrowed
    /// form *must* match those for the key type.
    pub fn contains_key(&self, key: &K) -> bool {
        self.guard.data.contains_key_unbounded(key)
    }
}
