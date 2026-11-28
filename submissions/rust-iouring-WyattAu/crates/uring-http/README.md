# uring-http

Single-thread **io_uring event loop with hand-written HTTP/1.1 parsing
and fixed-shape JSON emission** — built for a 1-vCPU server challenge
where every syscall and every memcpy is the bill.

Companion to [`sqlite-btree`](../sqlite-btree) (direct SQLite reads);
the transport lives in [`uring-kit`](https://github.com/WyattAu/uring-kit)
(ring lifecycle, registered buffers, scatter-gather sends).

## What's inside

- **HTTP/1.1 request parser** — total over arbitrary inputs, zero-copy
  (`Request` views into the receive buffer; method/target/authorization/
  content-length/body-start), keep-alive and pipeline-aware.
- **JSON emission** — byte-format writers (`write_decimal`,
  `write_escaped`) into caller-owned buffers: no serde, no allocation,
  exact output-size prepass available.
- **Router seam** — a `dispatch` function mapping (method, target) to
  routes or SPEC-exact error rows; total dispatch (proven in the
  parent repo's Lean targets).

## Validation

- Parser totality and the dispatch rows are **machine-checked** (Lean
  targets 1–4 in the parent `proof/` workspace).
- The full server passes a 42-case challenge conformance suite and a
  differential oracle; every performance change lands with a paired
  A/B measurement (docs/CLAIMS.md in the parent repo).

## Honest scope

This is a **fixed-shape response server**, not a general web
framework: the JSON emission assumes you know your output shape (that
is the point — precomputed skeletons, zero allocation). The parser is
general; the emitter is schema-aware.

MIT OR Apache-2.0.
