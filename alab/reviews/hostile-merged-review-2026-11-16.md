<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Hostile review and incoming evidence-reader verification

The hostile review of landed `35ff8fe4edbc8740383e1c2295eae781c26403d6`
found no concrete defect in its two documentation paths. The complete workflow
remains attributed to f42, native and remaining counts are inclusive, standalone
prune is separate, later affected selections retain their source and overlap,
and the failed optional Python 3.9 probe and certification hold remain explicit.
Independent review checked those claims against the retained framing and run
summaries without replaying the completed workflow or native prerequisites.

The next selected gap was the feature-evidence log reader. On source
`4a160d8320ff63958e13ff56317ab68da2644be8`, original controlled probe
625e98 exits zero and demonstrates the issue: an owned real FIFO leaves the
reader waiting until an owned writer connects, then `log_bytes` accepts the
written bytes with their declared hash. The child returns zero and its returned
length/hash match the actual payload. Six copied input files match their Git
source before and after the probe. This is a controlled public-reader
observation, not a production hang or complete feature-admission result.

Fresh admitted fetch 28005a/completion ce90ff returns zero and verifies main
`26b1956a73808493fd5d21e8552f6bebbf7764c2`. That main already contains the
bounded evidence-reader fix and its regression tests. Decision: adopt the
published shared implementation and verify it rather than create another
reader. The owned worktree was clean and advanced to that exact source.
The existing `file_bytes_snapshot` shares regular-file acquisition, required
native no-follow/nonblocking flags, an initial-size-plus-one read bound,
before/after descriptor identity and close handling with the hash reader.
Hashing stays streamed; byte reads remain uncached. Feature logs hash and parse
the same returned bytes. The intended absolute-alias and text/raw JSON policies
are preserved. Opened-descriptor snapshots do not establish whole-path custody.

Original fixed probe 83ddb1 exits zero on 26b. The same kind of owned FIFO is
refused with `ValueError` identifying a non-regular file, without any writer.
Its six copied source inputs are unchanged and Git-bound. The private probe
packets and proof SHA-256 values are:

- `codex-feature-evidence-fifo-proof-_87grwi0`:
  `d07d1eeb29ea1ca2ac933d13b05cbf7da2927afcf21c68eb02847303aa742315`.
- `codex-feature-evidence-fifo-fixed-c2qy_gm7`:
  `6aa7b5192157a4a872239047b1f2ff820d12efc917165149669479ba1ced379d`.

Managed Python 3.14 under `-I -B` passes all 340 distinct selected methods on
clean 26b. Original 57913/completion 49a1c2 covers 291 methods in 20 modules;
original 15482/completion 1c3eae covers 49 methods in four additional modules.
All 24 runner children return zero, without skips, failures or errors.
The selection includes feature/log/metadata/bundle contracts, focused result
parsing, the shared reader and existing hash consumers, qualification and
cleanup, upstream admission, catalog IDs, runtime dispatch, daily control and
build-cache registration. It exercises the changed helper dependencies in copied
fixtures without claiming a native lint-fixture run.

Each selected-run packet retains 957 exact Git source inputs and unchanged
Python/Git/Bash executable maps. The private packet indices are:

- `codex-wide-composition-python-pdzvlnl0`:
  `52ee35e8ffc2b1234e460559e39f32f11d09f5b72f1dbcecfd5346ea5c157195`.
- `codex-wide-composition-python-x7re6f2p`:
  `50cdfa21c4ab24985f85b035e49bf946eee8c4bff8d937c7aed00c5489376597`.

Independent audits a799cf, b1fc91 and cedae0 return zero and verify both
packet indices, source/tool bindings, the distinct 340-method union and both
limited FIFO probes. FIFO nodes are inspected with `lstat` only.

These selected Python and file-reader observations do not extend the earlier
complete-workflow result to 26b or qualify an application, package or adoption.
The existing application-certification and external-producer holds stand.
No Rust compiler, application or native adapter was invoked by these probes or
selected runner entry commands. The selected fixtures retain their own finite
local subprocess controls.

Independent source review found no concrete regression in the incoming reader,
its cache/cleanup behavior, parsing policy or updated fixture dependencies.
No duplicate production patch is justified. This commit records the hostile
review, adoption decision and measured closure of the FIFO gap. Publication
uses the maintained Agh publisher with a fresh literal main lease and retires
only this session's topic branch.
