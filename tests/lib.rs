use ev_slotmap::WriteHandle;
use one_way_slot_map::{define_key_type, SlotMap};
use std::sync::{Arc, Mutex, RwLock};
use threadpool::ThreadPool;

macro_rules! assert_match {
    ($x:expr, $p:pat) => {
        if let $p = $x {
        } else {
            panic!(concat!(stringify!($x), " did not match ", stringify!($p)));
        }
    };
}

define_key_type!(TestKey<()> : Default + Clone + Copy);

#[test]
fn it_works() {
    let x = 42;

    let (r, mut w) = ev_slotmap::new();

    // the map is uninitialized, so all lookups should return None
    assert_match!(r.get(&TestKey::default()), None);

    let key = w.insert((), x);

    assert_match!(r.get(&key), Some(_));
    assert_eq!(r.contains_key(&key), true);

    let x = 54;
    let y = 69;

    w.update(key.clone(), x);

    let key2 = w.insert((), y);

    assert_match!(r.get(&key), Some(_));
    assert_eq!(r.contains_key(&key), true);

    assert_match!(r.get(&key2), Some(_));
    w.remove(&key.clone());

    // but after the swap, the record is there!
    assert_match!(r.get(&key), None);
    assert_eq!(r.contains_key(&key), false);

    assert_match!(r.get(&key2), Some(_));
    assert_eq!(r.contains_key(&key2), true);

    w.clear();

    assert_match!(r.get(&key), None);
    assert_eq!(r.contains_key(&key), false);

    assert_match!(r.get(&key2), None);
    assert_eq!(r.contains_key(&key2), false);
}

#[test]
fn read_after_drop() {
    let x = 42;

    let (r, mut w) = ev_slotmap::new();

    // the map is uninitialized, so all lookups should return None
    assert_match!(r.get(&TestKey::default()), None);

    let key = w.insert((), x);
    assert_match!(r.get(&key), Some(_));

    // once we drop the writer, the readers should see empty maps
    drop(w);
    assert_match!(r.get(&key), None);
}

#[test]
fn test_consistency() {
    let (r, w) = ev_slotmap::new();

    let writer: Arc<Mutex<WriteHandle<TestKey, (), usize>>> =
        Arc::new(Mutex::new(w));

    let threads = 10;
    let insertions = 100;
    let writes = 100;
    let reads = 1_000;

    let pool = ThreadPool::new(threads);

    let mut keys = Vec::new();

    for _ in 0..insertions {
        keys.push(writer.lock().unwrap().insert((), 0));
    }

    for _ in 0..threads {
        let writer_clone = writer.clone();
        let reader_clone = r.clone();
        let keys_clone = keys.clone();

        pool.execute(move || {
            for w in 0..writes {
                for k in keys_clone.iter() {
                    let mut write_lock = writer_clone.lock().unwrap();
                    let old_val = *reader_clone.get(k).unwrap();
                    write_lock.update(*k, old_val + 1);
                }
                for _ in 0..reads {
                    for k in keys_clone.iter() {
                        assert!(*reader_clone.get(k).unwrap() > w);
                    }
                }
            }
        });
    }

    pool.join();

    for k in keys.iter() {
        assert_eq!(writes * threads, *r.get(k).unwrap());
    }
}

//#[test]
#[allow(dead_code)]
fn test_performance_against_std_rwlock() {
    let map: Arc<RwLock<SlotMap<TestKey, (), usize>>> =
        Arc::new(RwLock::new(SlotMap::new()));

    let threads = 10;
    let insertions = 1_000;
    let writes = 1_00;
    let reads = 1_000;

    let pool = ThreadPool::new(threads);

    let mut keys = Vec::new();

    for _ in 0..insertions {
        keys.push(map.write().unwrap().insert((), 0));
    }

    for _ in 0..threads {
        let map_clone = map.clone();
        let keys_clone = keys.clone();

        pool.execute(move || {
            for w in 0..writes {
                for k in keys_clone.iter() {
                    let mut write_lock = map_clone.write().unwrap();
                    let val = write_lock.get_mut(k).unwrap();
                    *val += 1;
                }
                for _ in 0..reads {
                    for k in keys_clone.iter() {
                        assert!(*map_clone.read().unwrap().get(k).unwrap() > w);
                    }
                }
            }
        });
    }

    pool.join();

    for k in keys.iter() {
        assert_eq!(writes * threads, *map.read().unwrap().get(k).unwrap());
    }
}

#[test]
fn test_iter() {
    let (r, w) = ev_slotmap::new();

    let writer: Arc<Mutex<WriteHandle<TestKey, (), usize>>> =
        Arc::new(Mutex::new(w));

    let insertions = 10000;

    let mut keys = Vec::new();

    for i in 0..insertions {
        keys.push(Some(writer.lock().unwrap().insert((), i)));
    }

    let read_ref = r.read().expect("Dude, where's my read ref");

    read_ref.iter().for_each(|v| {
        let old = keys.get_mut(*v).expect("Dude, where's my entry").take();
        assert!(old.is_some());
    });

    for key in keys {
        assert!(key.is_none());
    }
}
