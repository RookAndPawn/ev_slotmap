use super::{Operation, ShallowCopy};
use crate::inner::Inner;
use crate::read::ReadHandle;
use slotmap::Key;
use std::mem::ManuallyDrop;
use std::sync::atomic;
use std::sync::{Arc, MutexGuard};
use std::{fmt, mem, thread};

/// A handle that may be used to modify the eventually consistent map.
///
/// Note that any changes made to the map will not be made visible to readers until `refresh()` is
/// called.
///
/// When the `WriteHandle` is dropped, the map is immediately (but safely) taken away from all
/// readers, causing all future lookups to return `None`.
///
/// # Examples
/// ```
/// let x = ('x', 42);
///
/// let (r, mut w) = evmap::new();
///
/// // the map is uninitialized, so all lookups should return None
/// assert_eq!(r.get(&x.0).map(|rs| rs.len()), None);
///
/// w.refresh();
///
/// // after the first refresh, it is empty, but ready
/// assert_eq!(r.get(&x.0).map(|rs| rs.len()), None);
///
/// w.insert(x.0, x);
///
/// // it is empty even after an add (we haven't refresh yet)
/// assert_eq!(r.get(&x.0).map(|rs| rs.len()), None);
///
/// w.refresh();
///
/// // but after the swap, the record is there!
/// assert_eq!(r.get(&x.0).map(|rs| rs.len()), Some(1));
/// assert_eq!(r.get(&x.0).map(|rs| rs.iter().any(|v| v.0 == x.0 && v.1 == x.1)), Some(true));
/// ```
pub struct WriteHandle<K, V, M = ()>
where
    K: Eq + Clone + Key,
    V: Eq + ShallowCopy + Copy,
    M: 'static + Clone,
{
    epochs: crate::Epochs,
    w_handle: Option<Box<Inner<K, ManuallyDrop<V>, M>>>,
    last_op: Option<Operation<K, V>>,
    r_handle: ReadHandle<K, V, M>,
    last_epochs: Vec<usize>,
    meta: M,
}

impl<K, V, M> fmt::Debug for WriteHandle<K, V, M>
where
    K: Eq + Clone + fmt::Debug + Key,
    V: Eq + ShallowCopy + fmt::Debug + Copy,
    M: 'static + Clone + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteHandle")
            .field("epochs", &self.epochs)
            .field("w_handle", &self.w_handle)
            .field("last_op", &self.last_op)
            .field("r_handle", &self.r_handle)
            .field("meta", &self.meta)
            .finish()
    }
}

pub(crate) fn new<K, V, M>(
    w_handle: Inner<K, ManuallyDrop<V>, M>,
    epochs: crate::Epochs,
    r_handle: ReadHandle<K, V, M>,
) -> WriteHandle<K, V, M>
where
    K: Eq + Clone + Key,
    V: Eq + ShallowCopy + Copy,
    M: 'static + Clone,
{
    let m = w_handle.meta.clone();
    WriteHandle {
        epochs,
        w_handle: Some(Box::new(w_handle)),
        last_op: Default::default(),
        r_handle,
        last_epochs: Vec::new(),
        meta: m
    }
}

impl<K, V, M> Drop for WriteHandle<K, V, M>
where
    K: Eq + Clone + Key,
    V: Eq + ShallowCopy + Copy,
    M: 'static + Clone,
{
    fn drop(&mut self) {
        use std::ptr;

        // first, ensure both maps are up to date
        // (otherwise safely dropping deduplicated rows is a pain)
        self.refresh_with_operation(Operation::Clear);
        self.refresh_with_operation(Operation::Clear);

        // next, grab the read handle and set it to NULL
        let r_handle = self
            .r_handle
            .inner
            .swap(ptr::null_mut(), atomic::Ordering::Release);

        // now, wait for all readers to depart
        let epochs = Arc::clone(&self.epochs);
        let mut epochs = epochs.lock().unwrap();
        self.wait(&mut epochs);

        // ensure that the subsequent epoch reads aren't re-ordered to before the swap
        atomic::fence(atomic::Ordering::SeqCst);

        let w_handle = &mut self.w_handle.as_mut().unwrap().data;

        // all readers have now observed the NULL, so we own both handles.
        // all records are duplicated between w_handle and r_handle.
        // since the two maps are exactly equal, we need to make sure that we *don't* call the
        // destructors of any of the values that are in our map, as they'll all be called when the
        // last read handle goes out of scope. to do so, we first clear w_handle, which won't drop
        // any elements since its values are kept as ManuallyDrop:
        w_handle.clear();

        // then we transmute r_handle to remove the ManuallyDrop, and then drop it, which will free
        // all the records. this is safe, since we know that no readers are using this pointer
        // anymore (due to the .wait() following swapping the pointer with NULL).
        drop(unsafe { Box::from_raw(r_handle as *mut Inner<K, V, M>) });
    }
}

impl<K, V, M> WriteHandle<K, V, M>
where
    K: Eq + Clone + Key,
    V: Eq + ShallowCopy + Copy,
    M: 'static + Clone,
{
    fn wait(&mut self, epochs: &mut MutexGuard<'_, slab::Slab<Arc<atomic::AtomicUsize>>>) {
        let mut iter = 0;
        let mut starti = 0;
        let high_bit = 1usize << (mem::size_of::<usize>() * 8 - 1);
        // we're over-estimating here, but slab doesn't expose its max index
        self.last_epochs.resize(epochs.capacity(), 0);
        'retry: loop {
            // read all and see if all have changed (which is likely)
            for (ii, (ri, epoch)) in epochs.iter().enumerate().skip(starti) {
                // note that `ri` _may_ have been re-used since we last read into last_epochs.
                // this is okay though, as a change still implies that the new reader must have
                // arrived _after_ we did the atomic swap, and thus must also have seen the new
                // pointer.
                if self.last_epochs[ri] & high_bit != 0 {
                    // reader was not active right after last swap
                    // and therefore *must* only see new pointer
                    continue;
                }

                let now = epoch.load(atomic::Ordering::Acquire);
                if (now != self.last_epochs[ri]) | (now & high_bit != 0) | (now == 0) {
                    // reader must have seen last swap
                } else {
                    // reader may not have seen swap
                    // continue from this reader's epoch
                    starti = ii;

                    // how eagerly should we retry?
                    if iter != 20 {
                        iter += 1;
                    } else {
                        thread::yield_now();
                    }

                    continue 'retry;
                }
            }
            break;
        }
    }

    fn run_operation(target: &mut Box<Inner<K, ManuallyDrop<V>, M>>,
        op: &Operation<K,V>) -> Option<K>
    {
        use Operation::*;

        let mut result = None;

        match op {
            Add(value) => {
                result = Some(target.data.insert(ManuallyDrop::new(*value)));
            }
            Replace(key, value) => {
                let old_value = target.data
                    .get_mut(key.clone())
                    .expect("Tried to replace empty key");

                *old_value = ManuallyDrop::new(*value);
            }
            Remove(key) => {
                let _ = target.data.remove(key.clone());
            }
            Clear => {
                target.data.clear();
            }
        }

        result
    }


    /// refresh the write/read handle with the given operation
    fn refresh_with_operation(&mut self, mut op: Operation<K,V>) -> Option<K> {
        // we need to wait until all epochs have changed since the swaps *or* until a "finished"
        // flag has been observed to be on for two subsequent iterations (there still may be some
        // readers present since we did the previous refresh)
        //
        // NOTE: it is safe for us to hold the lock for the entire duration of the swap. we will
        // only block on pre-existing readers, and they are never waiting to push onto epochs
        // unless they have finished reading.
        let epochs = Arc::clone(&self.epochs);
        let mut epochs = epochs.lock().unwrap();

        self.wait(&mut epochs);

        let result = {

            // all the readers have left!
            // we can safely bring the w_handle up to date.
            let w_handle = self.w_handle.as_mut().unwrap();

            if let Some(last_op) = &self.last_op {
                Self::run_operation(w_handle, &last_op);
            }

            let result = Self::run_operation(w_handle, &mut op);

            self.last_op = Some(op);

            // ensure meta-information is up to date
            w_handle.meta = self.meta.clone();
            w_handle.mark_ready();

            // w_handle (the old r_handle) is now fully up to date!
            result
        };

        // at this point, we have exclusive access to w_handle, and it is up-to-date with all
        // writes. the stale r_handle is accessed by readers through an Arc clone of atomic pointer
        // inside the ReadHandle. oplog contains all the changes that are in w_handle, but not in
        // r_handle.
        //
        // it's now time for us to swap the maps so that readers see up-to-date results from
        // w_handle.

        // prepare w_handle
        let w_handle = self.w_handle.take().unwrap();
        let w_handle = Box::into_raw(w_handle);

        // swap in our w_handle, and get r_handle in return
        let r_handle = self
            .r_handle
            .inner
            .swap(w_handle, atomic::Ordering::Release);
        let r_handle = unsafe { Box::from_raw(r_handle) };

        // ensure that the subsequent epoch reads aren't re-ordered to before the swap
        atomic::fence(atomic::Ordering::SeqCst);

        for (ri, epoch) in epochs.iter() {
            self.last_epochs[ri] = epoch.load(atomic::Ordering::Acquire);
        }

        // NOTE: at this point, there are likely still readers using the w_handle we got
        self.w_handle = Some(r_handle);

        result
    }

    /// Set the metadata.
    ///
    /// Will only be visible to readers after the next call to `refresh()`.
    pub fn set_meta(&mut self, mut meta: M) -> M {
        mem::swap(&mut self.meta, &mut meta);
        meta
    }

    /// Add the given value to the value-bag of the given key.
    ///
    /// The updated value-bag will only be visible to readers after the next call to `refresh()`.
    pub fn insert(&mut self, v: V) -> K {
        self.refresh_with_operation(Operation::Add(v)).expect("No key returned on insert")
    }

    /// Replace the value-bag of the given key with the given value.
    ///
    /// Replacing the value will automatically deallocate any heap storage and place the new value
    /// back into the `SmallVec` inline storage. This can improve cache locality for common
    /// cases where the value-bag is only ever a single element.
    ///
    /// See [the doc section on this](./index.html#small-vector-optimization) for more information.
    ///
    /// The new value will only be visible to readers after the next call to `refresh()`.
    pub fn update(&mut self, k: K, v: V) {
        let _ = self.refresh_with_operation(Operation::Replace(k, v));
    }

    /// Clear the value-bag of the given key, without removing it.
    ///
    /// If a value-bag already exists, this will clear it but leave the
    /// allocated memory intact for reuse, or if no associated value-bag exists
    /// an empty value-bag will be created for the given key.
    ///
    /// The new value will only be visible to readers after the next call to `refresh()`.
    pub fn clear(&mut self) {
        let _ = self.refresh_with_operation(Operation::Clear);
    }

    /// Remove the given value from the value-bag of the given key.
    ///
    /// The updated value-bag will only be visible to readers after the next call to `refresh()`.
    pub fn remove(&mut self, k: K) {
        let _ = self.refresh_with_operation(Operation::Remove(k));
    }

}

// allow using write handle for reads
use std::ops::Deref;
impl<K, V, M> Deref for WriteHandle<K, V, M>
where
    K: Eq + Clone + Key,
    V: Eq + ShallowCopy + Copy,
    M: 'static + Clone,
{
    type Target = ReadHandle<K, V, M>;
    fn deref(&self) -> &Self::Target {
        &self.r_handle
    }
}
