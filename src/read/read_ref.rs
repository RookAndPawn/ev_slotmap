use super::ReadGuard;
use crate::{inner::Inner};
use slotmap::{SlotMap, Key};
use std::borrow::Borrow;
use std::mem::ManuallyDrop;

use super::user_friendly;

/// A live reference into the read half of an evmap.
///
/// As long as this lives, the map being read cannot change, and if a writer attempts to
/// call [`WriteHandle::refresh`], that call will block until this is dropped.
///
/// Since the map remains immutable while this lives, the methods on this type all give you
/// unguarded references to types contained in the map.
#[derive(Debug)]
pub struct MapReadRef<'rh, K, V, M = ()>
where
    K: Eq + Key,
    V: Eq + Copy,
{
    pub(super) guard: ReadGuard<'rh, Inner<K, ManuallyDrop<V>, M>>,
}

impl<'rh, K, V, M> MapReadRef<'rh, K, V, M>
where
    K: Eq + Key,
    V: Eq + Copy,
{

    /// Returns the number of non-empty keys present in the map.
    pub fn len(&self) -> usize {
        self.guard.data.len()
    }

    /// Returns true if the map contains no elements.
    pub fn is_empty(&self) -> bool {
        self.guard.data.is_empty()
    }

    /// Get the current meta value.
    pub fn meta(&self) -> &M {
        &self.guard.meta
    }

    /// Returns a reference to the values corresponding to the key.
    ///
    /// The key may be any borrowed form of the map's key type, but `Hash` and `Eq` on the borrowed
    /// form *must* match those for the key type.
    ///
    /// Note that not all writes will be included with this read -- only those that have been
    /// refreshed by the writer. If no refresh has happened, or the map has been destroyed, this
    /// function returns `None`.
    pub fn get<'a>(&'a self, key: K) -> Option<&'a V>
    {
        self.guard.data.get(key).map(user_friendly)
    }

    /// Returns true if the map contains any values for the specified key.
    ///
    /// The key may be any borrowed form of the map's key type, but `Hash` and `Eq` on the borrowed
    /// form *must* match those for the key type.
    pub fn contains_key(&self, key: K) -> bool
    {
        self.guard.data.contains_key(key)
    }
}
