# Compatibility sources

`allocative/` carries the exact crates.io `allocative 0.3.6` source used by the
committed Codex lockfile, with its upstream Apache and MIT licenses. The original
archive and file digests are in `allocative/ALAB-SOURCE.json`.

SDK545 treats `std::convert::Infallible` as an alias of `!`. Upstream 0.3.6 enables
both `Allocative` implementations on every dev/nightly compiler, which fails with
E0119. Our build script probes whether the two types alias and, when they do,
keeps the existing Infallible implementation as the implementation for both types.
It also probes whether `never_type` is stable before enabling that feature, avoiding
a `stable_features` warning on newer Trust compilers. Older compilers retain upstream
behavior. The unit regression requires both types to implement `Allocative` on
dev/nightly compilers.

The golden test helper also normalizes Rust's internal `alloc::rcs::arc::Arc` name
to its public `alloc::sync::Arc` spelling. Its allocation sizes, fields and edges
remain checked against the unchanged snapshot.

The workspace selects this copy with its `ALAB(toolchain)` patch entry. Drop this copy and entry when
upstream ships equivalent compatibility and Codex's committed lockfile selects it.

Pristine upstream (alab/check.sh's comparisons, alab/sync.sh's lane-fault build) is built
with the toolchain compatibility copies too, as path overrides (`lib.sh`, `alab_pristine_config`): without them
upstream alone does not compile under Trust. Each toolchain copy's `ALAB-SOURCE.json` names
its original archive, and a path override keeps upstream's dependencies.

`portable-pty/` carries the locked crates.io 0.9.0 library and its MIT license.
The one-line Unix change passes an explicit working directory to the child even
when the path is missing. The OS then reports the failed launch. An omitted
directory still defaults to the home directory. The receipt records the original
archive and file digests. Its `pristine_override` is false so upstream comparisons
use crates.io's original behavior. Retire the patch when the locked upstream
version requires an explicit working directory.
