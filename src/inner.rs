use evmap::ShallowCopy;
use one_way_slot_map::{define_key_type, SlotMap, SlotMapKey};
use std::fmt;
use std::mem::ManuallyDrop;

define_key_type!(pub(crate) InnerKey<()> : Copy + Clone);

/// Recast the given data as a map from the inner key type to the original
/// value. This is safe because SlotMap is repr(transparent) to a type that
/// does not include K or P
fn adapt_slot_map_key_type<K, P, V>(
    data: SlotMap<K, P, V>,
) -> SlotMap<InnerKey, (), V>
where
    K: SlotMapKey<P>,
{
    unsafe { std::mem::transmute(data) }
}

/// Recast the given data as a map from the original key type to a manually drop
/// value. This is safe because ManuallyDrop is repr(transparent) to the wrapped
/// type
fn adapt_slot_map_value_type<K, P, V>(
    data: SlotMap<K, P, V>,
) -> SlotMap<K, P, ManuallyDrop<V>>
where
    K: SlotMapKey<P>,
{
    unsafe { std::mem::transmute(data) }
}

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

impl<V> Inner<ManuallyDrop<V>>
where
    V: ShallowCopy,
{
    pub(crate) fn new() -> Self {
        Inner {
            data: SlotMap::new(),
            ready: false,
        }
    }

    pub(crate) fn new_with_data<K, P>(data: SlotMap<K, P, V>) -> (Self, Self)
    where
        K: SlotMapKey<P>,
    {
        let adapted_data = adapt_slot_map_key_type(data);
        let data1 = adapted_data.map(|v| unsafe { v.shallow_copy() });
        let data2 = adapt_slot_map_value_type(adapted_data);

        (
            Inner {
                data: data1,
                ready: true,
            },
            Inner {
                data: data2,
                ready: true,
            },
        )
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
