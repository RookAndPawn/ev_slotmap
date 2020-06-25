use super::Operation;
use crate::inner::{Inner, InnerKey};
use crate::read::ReadHandle;
use evmap::ShallowCopy;
use one_way_slot_map::SlotMapKey as Key;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::sync::atomic;
use std::sync::{Arc, MutexGuard};
use std::{fmt, mem, thread};

/// A handle that may be used to modify the concurrent map.
///
/// When the `WriteHandle` is dropped, the map is immediately (but safely) taken away from all
/// readers, causing all future lookups to return `None`.
///
/// ```
pub struct WriteHandle<K, P, V>
where
    K: Key<P>,
    V: ShallowCopy,
{
    epochs: crate::Epochs,
    w_handle: Option<Box<Inner<ManuallyDrop<V>>>>,
    last_op: Option<Operation<V>>,
    r_handle: ReadHandle<K, P, V>,
    last_epochs: Vec<usize>,

    phantom_p: PhantomData<P>,
}

impl<K, P, V> fmt::Debug for WriteHandle<K, P, V>
where
    K: Key<P> + fmt::Debug,
    V: fmt::Debug + ShallowCopy,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteHandle")
            .field("epochs", &self.epochs)
            .field("w_handle", &self.w_handle)
            .field("last_op", &self.last_op)
            .field("r_handle", &self.r_handle)
            .finish()
    }
}

pub(crate) fn new<K, P, V>(
    w_handle: Inner<ManuallyDrop<V>>,
    epochs: crate::Epochs,
    r_handle: ReadHandle<K, P, V>,
) -> WriteHandle<K, P, V>
where
    K: Key<P>,
    V: ShallowCopy,
{
    WriteHandle {
        epochs,
        w_handle: Some(Box::new(w_handle)),
        last_op: Default::default(),
        r_handle,
        last_epochs: Vec::new(),

        phantom_p: Default::default(),
    }
}

impl<K, P, V> Drop for WriteHandle<K, P, V>
where
    K: Key<P>,
    V: ShallowCopy,
{
    fn drop(&mut self) {
        use std::ptr;

        // first, ensure both maps are up to date
        // (otherwise safely dropping de-duplicated rows is a pain)
        while self.last_op.is_some() {
            self.refresh();
        }

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
        drop(unsafe { Box::from_raw(r_handle as *mut Inner<V>) });
    }
}

impl<K, P, V> WriteHandle<K, P, V>
where
    K: Key<P>,
    V: ShallowCopy,
{
    fn wait(
        &mut self,
        epochs: &mut MutexGuard<'_, slab::Slab<Arc<atomic::AtomicUsize>>>,
    ) {
        let mut iter = 0;
        let mut start_i = 0;
        let high_bit = 1usize << (mem::size_of::<usize>() * 8 - 1);
        // we're over-estimating here, but slab doesn't expose its max index
        self.last_epochs.resize(epochs.capacity(), 0);
        'retry: loop {
            // read all and see if all have changed (which is likely)
            for (ii, (ri, epoch)) in epochs.iter().enumerate().skip(start_i) {
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
                if (now != self.last_epochs[ri])
                    | (now & high_bit != 0)
                    | (now == 0)
                {
                    // reader must have seen last swap
                } else {
                    // reader may not have seen swap
                    // continue from this reader's epoch
                    start_i = ii;

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

    #[allow(clippy::borrowed_box)]
    fn run_operation_first(
        target: &mut Box<Inner<ManuallyDrop<V>>>,
        op: &Operation<V>,
    ) -> Option<InnerKey> {
        let mut result = None;

        match op {
            Operation::NoOp => (),
            Operation::Add(value) => {
                result = Some(
                    target.data.insert((), unsafe { value.shallow_copy() }),
                );
            }
            Operation::Replace(key, value) => {
                let old_value = target
                    .data
                    .get_mut_unbounded(key)
                    .expect("Tried to replace empty key");

                *old_value = unsafe { value.shallow_copy() };
            }
            Operation::Remove(key) => {
                let _ = target.data.remove_unbounded(key);
            }
            Operation::Clear => {
                target.data.clear();
            }
        }

        result
    }

    fn run_operation_second(target: &mut Inner<V>, op: Operation<V>) {
        match op {
            Operation::NoOp => (),
            Operation::Add(value) => {
                let _ = target.data.insert((), value);
            }
            Operation::Replace(key, value) => {
                let old_value = target
                    .data
                    .get_mut_unbounded(&key)
                    .expect("Tried to replace empty key");

                *old_value = value;
            }
            Operation::Remove(key) => {
                let _ = target.data.remove_unbounded(&key);
            }
            Operation::Clear => {
                target.data.clear();
            }
        }
    }

    /// refresh the write/read handle with the given operation
    fn refresh_with_operation(
        &mut self,
        op: Operation<V>,
    ) -> Option<InnerKey> {
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

            if let Some(last_op) = self.last_op.take() {
                Self::run_operation_second(
                    unsafe { w_handle.do_drop() },
                    last_op,
                );
            }

            if let Operation::NoOp = &op {
                None
            } else {
                let result = Self::run_operation_first(w_handle, &op);

                self.last_op = Some(op);

                w_handle.mark_ready();

                // w_handle (the old r_handle) is now fully up to date!
                result
            }
        };

        // at this point, we have exclusive access to w_handle, and it is up-to-date with all
        // writes. the stale r_handle is accessed by readers through an Arc clone of atomic pointer
        // inside the ReadHandle. op log contains all the changes that are in w_handle, but not in
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

    pub(crate) fn refresh(&mut self) {
        let _ = self.refresh_with_operation(Operation::NoOp);
    }

    /// Insert the given value into the slot map and return the associated key
    pub fn insert(&mut self, p: P, v: V) -> K {
        self.refresh_with_operation(Operation::Add(v))
            .expect("No key returned on insert")
            .to_outer_key(p)
    }

    /// Replace the value of the given key with the given value.
    pub fn update(&mut self, k: K, v: V) {
        let _ = self.refresh_with_operation(Operation::Replace(*k.borrow(), v));
    }

    /// Clear the slot map.
    pub fn clear(&mut self) {
        let _ = self.refresh_with_operation(Operation::Clear);
    }

    /// Remove the value from the map for the given key
    pub fn remove(&mut self, k: &K) {
        let _ = self.refresh_with_operation(Operation::Remove(*k.borrow()));
    }
}

// allow using write handle for reads
use std::ops::Deref;
impl<K, P, V> Deref for WriteHandle<K, P, V>
where
    K: Key<P>,
    V: ShallowCopy,
{
    type Target = ReadHandle<K, P, V>;
    fn deref(&self) -> &Self::Target {
        &self.r_handle
    }
}
