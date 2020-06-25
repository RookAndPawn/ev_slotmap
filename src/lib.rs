//! # ev_slotmap

//! A lock-free, concurrent slot map.

//! Most of this library is a rip off of [Jon Gjengset's evmap](https://docs.rs/evmap/10.0.2/evmap/)
//! but with a few notable simplifications
//!
//! - The value-bag map is replaced with a [one-way slotmap](https://docs.rs/one_way_slot_map/0.2.0/one_way_slot_map/)
//! - No batched edits (required because slot map keys need to be returned on insert)
//! - No associated metadata
//!
//! The core synchronization component's of evmap are still present. Out of
//! simplicity, we also use the [ShallowCopy](https://docs.rs/evmap/10.0.2/evmap/shallow_copy/trait.ShallowCopy.html)
//! straight out of evmap instead of copy-pasting it in. Also the following
//! blurb is almost straight from evmap.
//!
//! This map implementation allows reads and writes to execute entirely in parallel, with no
//! implicit synchronization overhead. Reads never take locks on their critical path, and neither
//! do writes assuming there is a single writer (multi-writer is possible using a `Mutex`), which
//! significantly improves performance under contention.
//!
//! Unlike evmap which provides eventual consistency following explicit `refresh`
//! calls, synchronization between reads and writers happens before write methods
//! return. For read-heavy workloads, the scheme used by this module is particularly
//! useful. Writers can afford to refresh after every write, which provides up-to-date
//! reads, and readers remain fast as they do not need to ever take locks.

#![warn(
    missing_docs,
    rust_2018_idioms,
    missing_debug_implementations,
    intra_doc_link_resolution_failure
)]
#![allow(clippy::type_complexity)]

use one_way_slot_map::{SlotMapKey as Key, SlotMapKeyData};
use std::sync::{atomic, Arc, Mutex};
mod inner;
use crate::inner::Inner;
use evmap::ShallowCopy;
use slab::Slab;
pub(crate) type Epochs = Arc<Mutex<Slab<Arc<atomic::AtomicUsize>>>>;

/// A pending map operation.
#[non_exhaustive]
#[derive(PartialEq, Eq, Debug)]
pub(crate) enum Operation<V> {
    /// Just do a refresh without altering the data
    NoOp,
    /// Replace the value for this key with this value.
    Replace(SlotMapKeyData, V),
    /// Add this value to the map.
    Add(V),
    /// Remove the value with this key from the map.
    Remove(SlotMapKeyData),
    /// Clear the map.
    Clear,
}

mod write;
pub use crate::write::WriteHandle;

mod read;
pub use crate::read::{MapReadRef, ReadGuard, ReadHandle, ReadHandleFactory};

/// Create an empty ev slotmap.
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
