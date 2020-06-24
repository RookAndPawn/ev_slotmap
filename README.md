# ev_slotmap

[![Crates.io](https://img.shields.io/crates/v/ev_slotmap.svg)](https://crates.io/crates/ev_slotmap)
[![Documentation](https://docs.rs/ev_slotmap/badge.svg)](https://docs.rs/ev_slotmap/)

A lock-free, concurrent slot map.

Most of this library is a rip off of [Jon Gjengset's evmap](https://docs.rs/evmap/10.0.2/evmap/) but with a few notable simplifications

- The value-bag map is replaced with a [one-way slotmap](https://docs.rs/one_way_slot_map/0.2.0/one_way_slot_map/)
- No batched edits (required because slot map keys need to be returned on insert)
- No associated metadata

The core synchronization component's of evmap are still present. Out of simplicity, we also use the [ShallowCopy](https://docs.rs/evmap/10.0.2/evmap/shallow_copy/trait.ShallowCopy.html) straight out of evmap instead of copy-pasting it in. Also the following blurb is almost straight from evmap.

This map implementation allows reads and writes to execute entirely in parallel, with no
implicit synchronization overhead. Reads never take locks on their critical path, and neither
do writes assuming there is a single writer (multi-writer is possible using a `Mutex`), which
significantly improves performance under contention.

Unlike evmap which provides eventual consistency following explicit `refresh` calls, synchronization between reads and writers happens before write methods return. For read-heavy workloads, the scheme used by this module is particularly useful. Writers can afford to refresh after every write, which provides up-to-date reads, and readers remain fast as they do not need to ever take locks.

## Performance

Benchmarks to come
