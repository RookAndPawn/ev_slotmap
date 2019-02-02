use std::hash::{BuildHasher, Hash};

use serde::{
    de::{Deserialize, Deserializer},
    ser::{Serialize, Serializer},
};

use super::{read, write, Inner, ReadHandle, ReadWriteHandle, ShallowCopy, WriteHandle};

impl<K, V, M, S> Serialize for ReadHandle<K, V, M, S>
where
    K: Eq + Hash + Clone + Serialize,
    S: BuildHasher + Clone,
    V: Eq + ShallowCopy + Serialize,
    M: 'static + Clone + Serialize,
{
    fn serialize<SER>(&self, serializer: SER) -> Result<SER::Ok, SER::Error>
    where
        SER: Serializer,
    {
        self.with_handle_raw(|inner| match inner {
            Some(inner) => serializer.serialize_some(inner),
            None => serializer.serialize_none(),
        })
    }
}

impl<K, V, M, S> Serialize for WriteHandle<K, V, M, S>
where
    K: Eq + Hash + Clone + Serialize,
    S: BuildHasher + Clone,
    V: Eq + ShallowCopy + Serialize,
    M: 'static + Clone + Serialize,
{
    #[inline]
    fn serialize<SER>(&self, serializer: SER) -> Result<SER::Ok, SER::Error>
    where
        SER: Serializer,
    {
        (**self).serialize(serializer)
    }
}

impl<K, V, M, S> Serialize for ReadWriteHandle<K, V, M, S>
where
    K: Eq + Hash + Clone + Serialize,
    S: BuildHasher + Clone,
    V: Eq + ShallowCopy + Serialize,
    M: 'static + Clone + Serialize,
{
    #[inline]
    fn serialize<SER>(&self, serializer: SER) -> Result<SER::Ok, SER::Error>
    where
        SER: Serializer,
    {
        (**self).serialize(serializer)
    }
}

impl<'de, K, V, M, S> Deserialize<'de> for WriteHandle<K, V, M, S>
where
    K: Eq + Hash + Clone + Deserialize<'de>,
    S: BuildHasher + Clone + Default,
    V: Eq + ShallowCopy + Deserialize<'de>,
    M: 'static + Clone + Deserialize<'de> + Default,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner: Option<Inner<K, V, M, S>> = Option::deserialize(deserializer)?;

        let inner = inner.unwrap_or_default();

        let mut w_handle = inner.clone();
        w_handle.mark_ready();

        let w = write::new(w_handle, read::new(inner));

        Ok(w)
    }
}

impl<'de, K, V, M, S> Deserialize<'de> for ReadWriteHandle<K, V, M, S>
where
    K: Eq + Hash + Clone + Deserialize<'de>,
    S: BuildHasher + Clone + Default,
    V: Eq + ShallowCopy + Deserialize<'de>,
    M: 'static + Clone + Deserialize<'de> + Default,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner: Option<Inner<K, V, M, S>> = Option::deserialize(deserializer)?;

        let inner = inner.unwrap_or_default();

        let mut w_handle = inner.clone();
        w_handle.mark_ready();

        let r = read::new(inner);
        let w = write::new(w_handle, r.clone());

        Ok(ReadWriteHandle::from_rw((r, w)))
    }
}
