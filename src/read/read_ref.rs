use super::ReadGuard;
use crate::{inner::Inner, values::Values};
use slotmap::{SlotMap, Key};
use std::borrow::Borrow;
use std::mem::ManuallyDrop;


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
    /// Iterate over all key + valuesets in the map.
    ///
    /// Be careful with this function! While the iteration is ongoing, any writer that tries to
    /// refresh will block waiting on this reader to finish.
    pub fn iter(&self) -> ReadGuardIter<'_, K, V> {
        ReadGuardIter {
            iter: Some(self.guard.data.iter()),
        }
    }

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
    pub fn get<'a, Q: ?Sized>(&'a self, key: &'_ Q) -> Option<&'a V>
    where
        K: Borrow<Q>,
        Q: Eq,
    {
        self.guard.data.get(key).map(Values::user_friendly)
    }

    /// Returns a guarded reference to _one_ value corresponding to the key.
    ///
    /// This is mostly intended for use when you are working with no more than one value per key.
    /// If there are multiple values stored for this key, there are no guarantees to which element
    /// is returned.
    ///
    /// The key may be any borrowed form of the map's key type, but `Hash` and `Eq` on the borrowed
    /// form *must* match those for the key type.
    ///
    /// Note that not all writes will be included with this read -- only those that have been
    /// refreshed by the writer. If no refresh has happened, or the map has been destroyed, this
    /// function returns `None`.
    pub fn get_one<'a, Q: ?Sized>(&'a self, key: &'_ Q) -> Option<&'a V>
    where
        K: Borrow<Q>,
        Q: Eq,
    {
        self.guard
            .data
            .get(key)
            .and_then(|values| values.user_friendly().get_one())
    }

    /// Returns true if the map contains any values for the specified key.
    ///
    /// The key may be any borrowed form of the map's key type, but `Hash` and `Eq` on the borrowed
    /// form *must* match those for the key type.
    pub fn contains_key<Q: ?Sized>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq,
    {
        self.guard.data.contains_key(key)
    }
}

impl<'rh, K, Q, V, M> std::ops::Index<&'_ Q> for MapReadRef<'rh, K, V, M>
where
    K: Eq + Borrow<Q> + Key,
    V: Eq + Copy,
    Q: Eq + ?Sized
{
    type Output = V;
    fn index(&self, key: &Q) -> &Self::Output {
        self.get(key).unwrap()
    }
}

impl<'rg, 'rh, K, V, M> IntoIterator for &'rg MapReadRef<'rh, K, V, M>
where
    K: Eq + Key,
    V: Eq + Copy,
{
    type Item = (&'rg K, &'rg V);
    type IntoIter = ReadGuardIter<'rg, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An [`Iterator`] over keys and values in the evmap.
#[derive(Debug)]
pub struct ReadGuardIter<'rg, K, V>
where
    K: Eq + Key,
    V: Eq + Copy,
{
    iter: Option<
        <&'rg SlotMap<K, ManuallyDrop<V>> as IntoIterator>::IntoIter,
    >,
}

impl<'rg, K, V> Iterator for ReadGuardIter<'rg, K, V>
where
    K: Eq + Key,
    V: Eq + Copy
{
    type Item = (&'rg K, &'rg V);
    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .as_mut()
            .and_then(|iter| iter.next().map(|(k, v)| (k, v.user_friendly())))
    }
}
