use super::ReadHandle;
use crate::inner::Inner;
use slotmap::Key;
use std::mem::ManuallyDrop;
use std::sync::atomic::AtomicPtr;
use std::{fmt, sync};

/// A type that is both `Sync` and `Send` and lets you produce new [`ReadHandle`] instances.
///
/// This serves as a handy way to distribute read handles across many threads without requiring
/// additional external locking to synchronize access to the non-`Sync` `ReadHandle` type. Note
/// that this _internally_ takes a lock whenever you call [`ReadHandleFactory::handle`], so
/// you should not expect producing new handles rapidly to scale well.
pub struct ReadHandleFactory<K, V, M = ()>
where
    K: Eq + Key,
    V: Copy
{
    pub(super) inner: sync::Arc<AtomicPtr<Inner<K, ManuallyDrop<V>, M>>>,
    pub(super) epochs: crate::Epochs,
}

impl<K, V, M> fmt::Debug for ReadHandleFactory<K, V, M>
where
    K: Eq + Key,
    V: Copy
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadHandleFactory")
            .field("epochs", &self.epochs)
            .finish()
    }
}

impl<K, V, M> Clone for ReadHandleFactory<K, V, M>
where
    K: Eq + Key,
    V: Copy
{
    fn clone(&self) -> Self {
        Self {
            inner: sync::Arc::clone(&self.inner),
            epochs: sync::Arc::clone(&self.epochs),
        }
    }
}

impl<K, V, M> ReadHandleFactory<K, V, M>
where
    K: Eq + Key,
    V: Copy
{
    /// Produce a new [`ReadHandle`] to the same map as this factory was originally produced from.
    pub fn handle(&self) -> ReadHandle<K, V, M> {
        ReadHandle::new(
            sync::Arc::clone(&self.inner),
            sync::Arc::clone(&self.epochs),
        )
    }
}
