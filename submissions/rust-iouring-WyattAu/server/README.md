# feed-server

The $12 Server Challenge reference implementation: five SPEC endpoints
served from a single-thread io_uring pass loop over a direct SQLite
B-tree reader.

This is a **reference binary**, not a library — the reusable parts are
the crates it composes:

- [`uring-http`](https://crates.io/crates/uring-http) — HTTP/1.1 parser + fixed-shape JSON emission
- [`sqlite-btree`](https://crates.io/crates/sqlite-btree) — read-only SQLite B-tree reader (mmap, WAL snapshots)
- [`uring-kit`](https://crates.io/crates/uring-kit) — io_uring socket substrate

Run: `SQLITE_PATH=app.db JWT_SECRET=... cargo run --release`.
Validation story (differential oracle, Lean bounds, 42-case
conformance, measured claims ledger) lives in the parent repo.
