use super::ReadHandle;
use crate::inner::Inner;
use one_way_slot_map::SlotMapKey as Key;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::sync::atomic::AtomicPtr;
use std::{fmt, sync};

/// A type that is both `Sync` and `Send` and lets you produce new [`ReadHandle`] instances.
///
/// This serves as a handy way to distribute read handles across many threads without requiring
/// additional external locking to synchronize access to the non-`Sync` `ReadHandle` type. Note
/// that this _internally_ takes a lock whenever you call [`ReadHandleFactory::handle`], so
/// you should not expect producing new handles rapidly to scale well.
pub struct ReadHandleFactory<K, P, V>
where
    K: Key<P>,
{
    pub(super) inner: sync::Arc<AtomicPtr<Inner<ManuallyDrop<V>>>>,
    pub(super) epochs: crate::Epochs,

    pub(super) _phantom_p: PhantomData<P>,
    pub(super) _phantom_k: PhantomData<K>,
}

impl<K, P, V> fmt::Debug for ReadHandleFactory<K, P, V>
where
    K: Key<P>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadHandleFactory")
            .field("epochs", &self.epochs)
            .finish()
    }
}

impl<K, P, V> Clone for ReadHandleFactory<K, P, V>
where
    K: Key<P>,
{
    fn clone(&self) -> Self {
        Self {
            inner: sync::Arc::clone(&self.inner),
            epochs: sync::Arc::clone(&self.epochs),

            _phantom_p: Default::default(),
            _phantom_k: Default::default(),
        }
    }
}

impl<K, P, V> ReadHandleFactory<K, P, V>
where
    K: Key<P>,
{
    /// Produce a new [`ReadHandle`] to the same map as this factory was originally produced from.
    pub fn handle(&self) -> ReadHandle<K, P, V> {
        ReadHandle::new(
            sync::Arc::clone(&self.inner),
            sync::Arc::clone(&self.epochs),
        )
    }
}
