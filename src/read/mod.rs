use crate::inner::Inner;
use one_way_slot_map::SlotMapKey as Key;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::sync::atomic;
use std::sync::atomic::AtomicPtr;
use std::sync::{self, Arc};
use std::{cell, fmt, mem};

mod guard;
pub use guard::ReadGuard;

mod factory;
pub use factory::ReadHandleFactory;

mod read_ref;
pub use read_ref::MapReadRef;

/// Turn an manually drop into something useable
pub(crate) fn user_friendly<'a, T>(to_fix: &'a ManuallyDrop<T>) -> &'a T {
    unsafe { &*(to_fix as *const ManuallyDrop<T> as *const T) }
}

/// A handle that may be used to read from the eventually consistent map.
///
/// Note that any changes made to the map will not be made visible until the writer calls
/// `refresh()`. In other words, all operations performed on a `ReadHandle` will *only* see writes
/// to the map that preceded the last call to `refresh()`.
pub struct ReadHandle<K, P, V>
where
    K: Key<P>,
{
    pub(crate) inner: sync::Arc<AtomicPtr<Inner<ManuallyDrop<V>>>>,
    pub(crate) epochs: crate::Epochs,
    epoch: sync::Arc<sync::atomic::AtomicUsize>,
    epoch_i: usize,
    my_epoch: sync::atomic::AtomicUsize,

    // Since a `ReadHandle` keeps track of its own epoch, it is not safe for multiple threads to
    // call `with_handle` at the same time. We *could* keep it `Sync` and make `with_handle`
    // require `&mut self`, but that seems overly excessive. It would also mean that all other
    // methods on `ReadHandle` would now take `&mut self`, *and* that `ReadHandle` can no longer be
    // `Clone`. Since opt-in_builtin_traits is still an unstable feature, we use this hack to make
    // `ReadHandle` be marked as `!Sync` (since it contains an `Cell` which is `!Sync`).
    _not_sync_no_feature: PhantomData<cell::Cell<()>>,

    _phantom_p: PhantomData<P>,
    _phantom_k: PhantomData<K>,
}

impl<K, P, V> Drop for ReadHandle<K, P, V>
where
    K: Key<P>,
{
    fn drop(&mut self) {
        // parity must be restored, so okay to lock since we're not holding up the epoch
        let e = self.epochs.lock().unwrap().remove(self.epoch_i);
        assert!(Arc::ptr_eq(&e, &self.epoch));
    }
}

impl<K, P, V> fmt::Debug for ReadHandle<K, P, V>
where
    K: fmt::Debug + Key<P>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadHandle")
            .field("epochs", &self.epochs)
            .field("epoch", &self.epoch)
            .field("my_epoch", &self.my_epoch)
            .finish()
    }
}

impl<K, P, V> Clone for ReadHandle<K, P, V>
where
    K: Key<P>,
{
    fn clone(&self) -> Self {
        ReadHandle::new(
            sync::Arc::clone(&self.inner),
            sync::Arc::clone(&self.epochs),
        )
    }
}

pub(crate) fn new<K, P, V>(
    inner: Inner<ManuallyDrop<V>>,
    epochs: crate::Epochs,
) -> ReadHandle<K, P, V>
where
    K: Key<P>,
{
    let store = Box::into_raw(Box::new(inner));
    ReadHandle::new(sync::Arc::new(AtomicPtr::new(store)), epochs)
}

impl<K, P, V> ReadHandle<K, P, V>
where
    K: Key<P>,
{
    fn new(inner: sync::Arc<AtomicPtr<Inner<ManuallyDrop<V>>>>, epochs: crate::Epochs) -> Self {
        // tell writer about our epoch tracker
        let epoch = sync::Arc::new(atomic::AtomicUsize::new(0));
        // okay to lock, since we're not holding up the epoch
        let epoch_i = epochs.lock().unwrap().insert(Arc::clone(&epoch));

        Self {
            epochs,
            epoch,
            epoch_i,
            my_epoch: atomic::AtomicUsize::new(0),
            inner,
            _not_sync_no_feature: PhantomData,
            _phantom_p: Default::default(),
            _phantom_k: Default::default(),
        }
    }

    /// Create a new `Sync` type that can produce additional `ReadHandle`s for use in other
    /// threads.
    pub fn factory(&self) -> ReadHandleFactory<K, P, V> {
        ReadHandleFactory {
            inner: sync::Arc::clone(&self.inner),
            epochs: sync::Arc::clone(&self.epochs),
            _phantom_p: Default::default(),
            _phantom_k: Default::default(),
        }
    }
}

impl<K, P, V> ReadHandle<K, P, V>
where
    K: Key<P>,
{
    fn handle(&self) -> Option<ReadGuard<'_, Inner<ManuallyDrop<V>>>> {
        // once we update our epoch, the writer can no longer do a swap until we set the MSB to
        // indicate that we've finished our read. however, we still need to deal with the case of a
        // race between when the writer reads our epoch and when they decide to make the swap.
        //
        // assume that there is a concurrent writer. it just swapped the atomic pointer from A to
        // B. the writer wants to modify A, and needs to know if that is safe. we can be in any of
        // the following cases when we atomically swap out our epoch:
        //
        //  1. the writer has read our previous epoch twice
        //  2. the writer has already read our previous epoch once
        //  3. the writer has not yet read our previous epoch
        //
        // let's discuss each of these in turn.
        //
        //  1. since writers assume they are free to proceed if they read an epoch with MSB set
        //     twice in a row, this is equivalent to case (2) below.
        //  2. the writer will see our epoch change, and so will assume that we have read B. it
        //     will therefore feel free to modify A. note that *another* pointer swap can happen,
        //     back to A, but then the writer would be block on our epoch, and so cannot modify
        //     A *or* B. consequently, using a pointer we read *after* the epoch swap is definitely
        //     safe here.
        //  3. the writer will read our epoch, notice that MSB is not set, and will keep reading,
        //     continuing to observe that it is still not set until we finish our read. thus,
        //     neither A nor B are being modified, and we can safely use either.
        //
        // in all cases, using a pointer we read *after* updating our epoch is safe.

        // so, update our epoch tracker.
        let epoch = self.my_epoch.fetch_add(1, atomic::Ordering::Relaxed);
        self.epoch.store(epoch + 1, atomic::Ordering::Release);

        // ensure that the pointer read happens strictly after updating the epoch
        atomic::fence(atomic::Ordering::SeqCst);

        // then, atomically read pointer, and use the map being pointed to
        let r_handle = self.inner.load(atomic::Ordering::Acquire);

        // since we bumped our epoch, this pointer will remain valid until we bump it again
        let r_handle = unsafe { r_handle.as_ref() };

        if let Some(r_handle) = r_handle {
            // add a guard to ensure we restore read parity even if we panic
            Some(ReadGuard {
                handle: &self.epoch,
                epoch,
                t: r_handle,
            })
        } else {
            // the map has not yet been initialized, so restore parity and return None
            self.epoch.store(
                (epoch + 1) | 1usize << (mem::size_of::<usize>() * 8 - 1),
                atomic::Ordering::Release,
            );
            None
        }
    }

    /// Take out a guarded live reference to the read side of the map.
    ///
    /// This lets you perform more complex read operations on the map.
    ///
    /// While the reference lives, the map cannot be refreshed.
    ///
    /// If no refresh has happened, or the map has been destroyed, this function returns `None`.
    ///
    /// See [`MapReadRef`].
    pub fn read(&self) -> Option<MapReadRef<'_, K, P, V>> {
        let guard = self.handle()?;
        if !guard.is_ready() {
            return None;
        }
        Some(MapReadRef {
            guard,
            _phantom_k: Default::default(),
            _phantom_p: Default::default(),
        })
    }

    /// Returns the number of non-empty keys present in the map.
    pub fn len(&self) -> usize {
        self.read().map_or(0, |x| x.len())
    }

    /// Returns true if the map contains no elements.
    pub fn is_empty(&self) -> bool {
        self.read().map_or(true, |x| x.is_empty())
    }

    /// Internal version of `get_and`
    fn get_raw(&self, key: &K) -> Option<ReadGuard<'_, ManuallyDrop<V>>> {
        let inner = self.handle()?;
        if !inner.is_ready() {
            return None;
        }
        inner.map_opt(|inner| inner.data.get_unbounded(key))
    }

    /// Returns a guarded reference to the values corresponding to the key.
    ///
    /// While the guard lives, the map cannot be refreshed.
    ///
    /// The key may be any borrowed form of the map's key type, but `Hash` and `Eq` on the borrowed
    /// form must match those for the key type.
    ///
    /// Note that not all writes will be included with this read -- only those that have been
    /// refreshed by the writer. If no refresh has happened, or the map has been destroyed, this
    /// function returns `None`.
    #[inline]
    pub fn get<'rh>(&'rh self, key: &K) -> Option<ReadGuard<'rh, V>> {
        // call `borrow` here to monomorphize `get_raw` fewer times
        Some(self.get_raw(key)?.map_ref(user_friendly))
    }

    /// Returns true if the writer has destroyed this map.
    ///
    /// See [`WriteHandle::destroy`].
    pub fn is_destroyed(&self) -> bool {
        self.handle().is_none()
    }

    /// Returns true if the map contains any values for the specified key.
    ///
    /// The key may be any borrowed form of the map's key type, but `Hash` and `Eq` on the borrowed
    /// form *must* match those for the key type.
    pub fn contains_key(&self, key: &K) -> bool {
        self.read().map_or(false, |x| x.contains_key(key))
    }
}
