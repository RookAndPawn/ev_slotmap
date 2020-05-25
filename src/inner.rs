use crate::values::Values;
use slotmap::{SlotMap, Key};
use std::fmt;
use std::mem::ManuallyDrop;

pub(crate) struct Inner<K, V, M>
where
    K: Eq + Key,
    V: Copy
{
    pub(crate) data: SlotMap<K, V>,
    pub(crate) meta: M,
    ready: bool,
}

impl<K, V, M> Inner<K, ManuallyDrop<V>, M>
where
    K: Eq + Key,
    V: Copy
{
    pub(crate) unsafe fn do_drop(&mut self) -> &mut Inner<K, V, M> {
        &mut *(self as *mut Self as *mut Inner<K, V, M>)
    }
}

impl<K, V, M> fmt::Debug for Inner<K, V, M>
where
    K: Eq + Key + fmt::Debug,
    V: fmt::Debug + Copy,
    M: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("data", &self.data)
            .field("meta", &self.meta)
            .field("ready", &self.ready)
            .finish()
    }
}

impl<K, V, M> Clone for Inner<K, V, M>
where
    K: Eq + Key +Clone,
    M: Clone,
    V: Clone + Copy
{
    fn clone(&self) -> Self {
        assert!(self.data.is_empty());
        Inner {
            data: self.data.clone(),
            meta: self.meta.clone(),
            ready: self.ready,
        }
    }
}

impl<K, V, M> Inner<K, ManuallyDrop<V>, M>
where
    K: Eq + Key,
    V: Copy
{

    pub fn with_capacity(m: M, capacity: usize) -> Self {
        Inner {
            data: SlotMap::with_capacity_and_key(capacity),
            meta: m,
            ready: false,
        }
    }
}

impl<K, V, M> Inner<K, V, M>
where
    K: Eq + Key,
    V: Copy
{
    pub fn mark_ready(&mut self) {
        self.ready = true;
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }
}
