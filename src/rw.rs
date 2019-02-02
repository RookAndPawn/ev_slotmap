use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash};
use std::ops::Deref;

use parking_lot;

use super::{Options, ReadHandle, ShallowCopy, WriteHandle};

/// Contains both a `ReadHandle` and mutex-protected `WriteHandle`
pub struct ReadWriteHandle<K, V, M = (), S = RandomState>
where
    K: Eq + Hash + Clone,
    S: BuildHasher + Clone,
    V: Eq + ShallowCopy,
    M: 'static + Clone,
{
    r_handle: ReadHandle<K, V, M, S>,
    w_handle: parking_lot::Mutex<WriteHandle<K, V, M, S>>,
}

impl<K, V, M, S> Deref for ReadWriteHandle<K, V, M, S>
where
    K: Eq + Hash + Clone,
    S: BuildHasher + Clone,
    V: Eq + ShallowCopy,
    M: 'static + Clone,
{
    type Target = ReadHandle<K, V, M, S>;

    fn deref(&self) -> &ReadHandle<K, V, M, S> {
        &self.r_handle
    }
}

impl<K, V, M, S> ReadWriteHandle<K, V, M, S>
where
    K: Eq + Hash + Clone,
    S: BuildHasher + Clone,
    V: Eq + ShallowCopy,
    M: 'static + Clone,
{
    /// Construct a new `ReadWriteHandle` from individual read/write handles
    #[cfg_attr(feature = "cargo-clippy", allow(type_complexity))]
    pub fn from_rw(
        (r_handle, w_handle): (ReadHandle<K, V, M, S>, WriteHandle<K, V, M, S>),
    ) -> ReadWriteHandle<K, V, M, S> {
        ReadWriteHandle {
            r_handle,
            w_handle: parking_lot::Mutex::new(w_handle),
        }
    }

    /// Lock the write handle for writing.
    ///
    /// E.g.:
    ///
    /// ```ignore
    /// map.write(|w| {
    ///     w.insert(0, 'a')
    ///      .insert(1, 'b')
    ///      .insert(2, 'c')
    ///      .refresh();
    /// });
    /// ```
    pub fn write<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&mut WriteHandle<K, V, M, S>) -> T,
    {
        let mut w_handle = self.w_handle.lock();

        f(&mut *w_handle)
    }

    /// Clone the internal `ReadHandle`, allowing it to be easily sent to other threads.
    #[inline]
    pub fn reader(&self) -> ReadHandle<K, V, M, S> {
        self.r_handle.clone()
    }
}

impl<K, V, M, S> Default for ReadWriteHandle<K, V, M, S>
where
    K: Eq + Hash + Clone,
    S: BuildHasher + Clone + Default,
    V: Eq + ShallowCopy,
    M: 'static + Clone + Default,
{
    fn default() -> Self {
        ReadWriteHandle::from_rw(
            Options::default()
                .with_hasher(<S as Default>::default())
                .with_meta(<M as Default>::default())
                .construct(),
        )
    }
}

impl<K, V, M, S> Extend<(K, V)> for ReadWriteHandle<K, V, M, S>
where
    K: Eq + Hash + Clone,
    S: BuildHasher + Clone,
    V: Eq + ShallowCopy,
    M: 'static + Clone,
{
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        // do this here to simplify monomorphisms
        // of `write` and `WriteHandle::extend`
        let iter = iter.into_iter();

        self.write(|w| w.extend(iter));
    }
}
