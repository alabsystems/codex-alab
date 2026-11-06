# Compatibility sources

`allocative/` carries the exact crates.io `allocative 0.3.6` source used by the
committed Codex lockfile, with its upstream Apache and MIT licenses. The original
archive and file digests are in `allocative/ALAB-SOURCE.json`.

SDK545 treats `std::convert::Infallible` as an alias of `!`. Upstream 0.3.6 enables
both `Allocative` implementations on every dev/nightly compiler, which fails with
E0119. Our build script probes whether the two types alias and, when they do,
keeps the existing Infallible implementation as the implementation for both types.
Older compilers retain upstream behavior. The unit regression requires both types
to implement `Allocative` on dev/nightly compilers.

The golden test helper also normalizes Rust's internal `alloc::rcs::arc::Arc` name
to its public `alloc::sync::Arc` spelling. Its allocation sizes, fields and edges
remain checked against the unchanged snapshot.

The workspace has one `ALAB(toolchain)` patch entry. Drop this copy and entry when
upstream ships equivalent compatibility and Codex's committed lockfile selects it.
