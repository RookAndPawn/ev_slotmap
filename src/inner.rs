use one_way_slot_map::{define_key_type, SlotMap, SlotMapKey};

use std::fmt;
use std::mem::ManuallyDrop;

define_key_type!(pub(crate) InnerKey<()> : Copy + Clone);

impl InnerKey {
    pub(crate) fn to_outer_key<K, P>(self, embedded: P) -> K
    where
        K: SlotMapKey<P>,
    {
        K::from((embedded, self.slot_key))
    }
}

pub(crate) struct Inner<V> {
    pub(crate) data: SlotMap<InnerKey, (), V>,
    ready: bool,
}

impl<V> Inner<ManuallyDrop<V>> {
    pub(crate) unsafe fn do_drop(&mut self) -> &mut Inner<V> {
        &mut *(self as *mut Self as *mut Inner<V>)
    }
}

impl<V> fmt::Debug for Inner<V>
where
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("data", &self.data)
            .field("ready", &self.ready)
            .finish()
    }
}

impl<V> Clone for Inner<V> {
    fn clone(&self) -> Self {
        assert!(self.data.is_empty());
        Inner {
            data: SlotMap::new(),
            ready: self.ready,
        }
    }
}

impl<V> Inner<ManuallyDrop<V>> {
    pub(crate) fn new() -> Self {
        Inner {
            data: SlotMap::new(),
            ready: false,
        }
    }
}

impl<V> Inner<V> {
    pub(crate) fn mark_ready(&mut self) {
        self.ready = true;
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.ready
    }
}
