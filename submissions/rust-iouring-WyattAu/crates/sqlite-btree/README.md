# sqlite-btree

Pure-Rust, read-only **SQLite B-tree reader over mmap** — no libsqlite,
no C, no SQL parser. Open a database file, walk table B-trees and
indexes directly, decode records zero-copy.

Part of the [$12 Server Challenge](https://benchmarks.arjaythedev.com/12-dollar-server)
submission, where the hot read path could not afford a SQL layer.

## What's inside

| Module | What | Notes |
|---|---|---|
| `varint` | SQLite varint decode (1–9 bytes) | `no_std`, total, Lean-bound |
| `record` | serial-type record header decode, zero-copy `Field` views | `no_std`, total, Lean-bound |
| `btree` | table cursor: seek/step/prev/payload over interior+leaf pages | `reader` feature |
| `scan` | index scans: lower_bound, sequential advance, batched per-id counting | `reader` feature |
| `mmap` | read-only mmap wrapper, hugepage-advised | `reader` feature |
| `wal` | WAL frame index (salts, frame count, snapshot scan) | `reader` feature |
| `store` | db+WAL snapshot management, page-at-snapshot reads | `reader` feature |

The decoders are `no_std` pure core; the mmap reader is behind the
default `reader` feature (std + memmap2, Linux/64-bit by declaration).

## How it is validated

This crate does not ask you to trust it:

1. **Differential oracle** (`tests/query_oracle.rs`): every read pattern
   must agree with **rusqlite** (bundled amalgamation) on the challenge
   seed, on randomized write batches, across WAL growth, checkpoint
   truncation and salt restarts. Green on every change — the gate is
   mandatory per the project's CLAIMS protocol.
2. **Varint oracle** (`tests/varint_oracle.rs` + proptest regressions):
   total decode, ≤ 9 bytes consumed, byte-for-byte agreement with
   SQLite's `getVarint`.
3. **Lean machine-checked bounds**: record-header field counts and value
   bounds are proved, not asserted (proof/ workspace of the parent).

## Honest limitations (by design)

- **Read-only**: writes go through a rusqlite writer connection (the
  split keeps the reader mmap-simple and the writer battle-tested).
- **Overflow cells rejected** (`Unsupported`): bodies long enough to
  overflow pages are not walked — callers route them to the fallback
  writer path. Documented, not hidden.
- **Linux / 64-bit**: the mmap substrate declares it.
- Type variety matches the target schema family: integer rowids,
  TEXT bodies, no fancy column types yet.

## Usage

```rust
use sqlite_btree::store::Store;
use sqlite_btree::btree::{Cursor, TableSeek};

let store = Store::open("app.db")?;
store.refresh()?; // advance the snapshot to the WAL's current end
let r = store.reader().ok_or(Unsupported)?;
let mut cur = Cursor::table(r, roots.posts)?;
cur.table_seek(rowid)?;
let payload = cur.payload()?; // zero-copy bytes at this snapshot
```

See the parent repo ([uring-http-kit](https://github.com/WyattAu/uring-http-kit))
for the differential-oracle harness and the performance claims ledger
(docs/CLAIMS.md — measured, not marketing).

MIT OR Apache-2.0.
