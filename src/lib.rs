//! A lock-free, eventually consistent, concurrent multi-value map.
//!
//! This map implementation allows reads and writes to execute entirely in parallel, with no
//! implicit synchronization overhead. Reads never take locks on their critical path, and neither
//! do writes assuming there is a single writer (multi-writer is possible using a `Mutex`), which
//! significantly improves performance under contention.
//!
//! The trade-off exposed by this module is one of eventual consistency: writes are not visible to
//! readers except following explicit synchronization. Specifically, readers only see the
//! operations that preceeded the last call to `WriteHandle::refresh` by a writer. This lets
//! writers decide how stale they are willing to let reads get. They can refresh the map after
//! every write to emulate a regular concurrent `HashMap`, or they can refresh only occasionally to
//! reduce the synchronization overhead at the cost of stale reads.
//!
//! For read-heavy workloads, the scheme used by this module is particularly useful. Writers can
//! afford to refresh after every write, which provides up-to-date reads, and readers remain fast
//! as they do not need to ever take locks.
//!
//! The map is multi-value, meaning that every key maps to a *collection* of values. This
//! introduces some memory cost by adding a layer of indirection through a `Vec` for each value,
//! but enables more advanced use. This choice was made as it would not be possible to emulate such
//! functionality on top of the semantics of this map (think about it -- what would the operational
//! log contain?).
//!
//! To faciliate more advanced use-cases, each of the two maps also carry some customizeable
//! meta-information. The writers may update this at will, and when a refresh happens, the current
//! meta will also be made visible to readers. This could be useful, for example, to indicate what
//! time the refresh happened.
//!
#![warn(
    missing_docs,
    rust_2018_idioms,
    missing_debug_implementations,
    intra_doc_link_resolution_failure
)]
#![allow(clippy::type_complexity)]

use one_way_slot_map::SlotMapKey as Key;
use std::fmt;
use std::sync::{atomic, Arc, Mutex};
mod inner;
use crate::inner::Inner;
use slab::Slab;
use evmap::ShallowCopy;
pub(crate) type Epochs = Arc<Mutex<Slab<Arc<atomic::AtomicUsize>>>>;

/// Unary predicate used to retain elements.
///
/// The predicate function is called once for each distinct value, and `true` if this is the
/// _first_ call to the predicate on the _second_ application of the operation.
pub struct Predicate<V>(pub(crate) Box<dyn FnMut(&V, bool) -> bool + Send>);

impl<V> Predicate<V> {
    /// Evaluate the predicate for the given element
    #[inline]
    pub fn eval(&mut self, value: &V, reset: bool) -> bool {
        (*self.0)(value, reset)
    }
}

impl<V> PartialEq for Predicate<V> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        // only compare data, not vtable: https://stackoverflow.com/q/47489449/472927
        &*self.0 as *const _ as *const () == &*other.0 as *const _ as *const ()
    }
}

impl<V> Eq for Predicate<V> {}

impl<V> fmt::Debug for Predicate<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Predicate")
            .field(&format_args!("{:p}", &*self.0 as *const _))
            .finish()
    }
}

/// A pending map operation.
#[non_exhaustive]
#[derive(PartialEq, Eq, Debug)]
pub enum Operation<K, V> {
    /// Just do a refresh without altering the data
    NoOp,
    /// Replace the value for this key with this value.
    Replace(K, V),
    /// Add this value to the map.
    Add(V),
    /// Remove the value with this key from the map.
    Remove(K),
    /// Clear the map.
    Clear,
}

mod write;
pub use crate::write::WriteHandle;

mod read;
pub use crate::read::{MapReadRef, ReadGuard, ReadHandle, ReadHandleFactory};

/// Create an empty eventually consistent map.
///
/// Use the [`Options`](./struct.Options.html) builder for more control over initialization.
#[allow(clippy::type_complexity)]
pub fn new<K, P, V>() -> (ReadHandle<K, P, V>, WriteHandle<K, P, V>)
where
    K: Key<P>,
    V: ShallowCopy,
{
    let epochs = Default::default();
    let inner = Inner::new();

    let mut w_handle = inner.clone();
    w_handle.mark_ready();
    let r = read::new(inner, Arc::clone(&epochs));
    let w = write::new(w_handle, epochs, r.clone());
    (r, w)
}
