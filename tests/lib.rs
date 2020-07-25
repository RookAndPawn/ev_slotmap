use ev_slotmap::WriteHandle;
use one_way_slot_map::{define_key_type, SlotMap};
use std::cell::RefCell;
use std::rc::Rc;
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
    let mut data = SlotMap::new();
    let mut keys = Vec::new();

    let insertions = 100;
    for _ in 0..insertions {
        keys.push(data.insert((), 0));
    }

    let (r, w) = ev_slotmap::new_with_data(data);

    let writer: Arc<Mutex<WriteHandle<TestKey, (), usize>>> =
        Arc::new(Mutex::new(w));

    let threads = 1;
    let writes = 100;
    let reads = 1_000;

    let pool = ThreadPool::new(threads);

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

struct DropCheckingType<'a, F>
where
    F: Fn(usize),
{
    index: usize,
    inner: &'a F,
}

impl<'a, F> Drop for DropCheckingType<'a, F>
where
    F: Fn(usize),
{
    fn drop(&mut self) {
        (self.inner)(self.index);
    }
}

#[test]
fn test_for_dropping_sanity() {
    let drop_check = Rc::new(RefCell::new(Vec::new()));
    let value_count = 1000;

    let drop_checker = |index| {
        let mut v = drop_check.borrow_mut();
        let c = v.get_mut(index).unwrap();
        *c += 1;
    };

    {
        let (r, mut w) = ev_slotmap::new();
        let mut keys = Vec::<TestKey>::new();
        for i in 0..value_count {
            drop_check.borrow_mut().push(0);
            keys.push(w.insert(
                (),
                Box::new(DropCheckingType {
                    index: i,
                    inner: &drop_checker,
                }),
            ));
        }
    }

    drop_check.borrow().iter().for_each(|v| assert_eq!(*v, 1));
}
