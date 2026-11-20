<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Artifact cancellation classification uses the actual exception type

Artifact inspection gives a pending cancellation precedence over later cleanup
stops and promotes a cleanup or note cancellation over an ordinary error.
Its `isinstance(error, Exception)` checks could execute an exception's custom
`__class__` accessor. A KeyboardInterrupt subtype reporting ValueError lost its
pending identity or had its cleanup/note cancellation swallowed; an accessor
raising SystemExit(0) replaced the selected exception entirely.

Decision: use `issubclass(type(error), Exception)` at both cancellation
classifications in `_close_descriptor`. Native type lineage determines which
exception owns cancellation without invoking that accessor. The existing
cleanup order, close-once behavior, exception chaining, ordinary error reports
and deliberate note-cancellation promotion policy remain unchanged. This
changes only the ALab-owned observer; it adds no source or native authority.

Four maintained public-observation controls inject the fault through the existing
owned-descriptor package fixture. They cover pending, cleanup and note
cancellations, plus a pending cancellation followed by a note stop. Each has a
native KeyboardInterrupt control and two subtypes: one reports an ordinary
class, and one refuses class observation with SystemExit(0). The fixture uses
real descriptor acquisition and closes each owned descriptor once, in reverse
order. The package executable is inert and never run.

On base `d8dd5e00e2b8930ee444b83d6ea994ff5d7ea34e`, managed Python 3.14.8 ran the
four new methods against unchanged production code: actual exit 1, eight
assertion failures, zero errors/skips. Each native control passed. The same
final test bytes then ran with the fix in four complete affected modules:
artifact status, cleanup, diagnostics and upstream status. All 81 distinct
methods passed, actual exit 0 in 6.480 seconds, with zero failures, errors,
skips, expected failures or unexpected successes. HEAD and hashes of all 211
top-level ALab and maintained-test Python sources were identical before and
after that run; the fixture scratch was empty afterward. `git diff --check`
also passed. Existing process-image tests mock ps/lsof; this is Python/local-Git
fixture verification, with no Rust build or native gate claim.

Evidence is retained under
`~/.codex/.ubd/artifact-lineage-review-pufgf7c7/`: `baseline-314.json`/`.log`,
`full-314.json`/`.log`, exact unique test IDs and source bookends. Earlier
exploratory logs in that directory used unsupported system Python 3.9 and are
not the validation reported here. Production SHA-256 is
`1990a157089a525ea931e100cb6de47bc654a758608732042a1f440497782e88`;
final test SHA-256 is
`3ffda514f49b56c08c31e8111f2209064d65b1d1812b60c275a115e14ee2c878`.
The full-run log SHA-256 is
`2128d5e37668b555dbfbaf9f45f2c11825fc0f1b0b9d8e3c5e587dba55aef50f`.

Before committing, the isolated checkout fast-forwarded to incoming main
`e056216d3bd5248c6d249281c58af594f497d281`. That commit changes only
`unit_results.py`, its tests and its review note; the four tested observer
modules and their implementation dependencies are unchanged. The 81-method
result above remains a run at `d8dd5e00`, not a relabeled run at the newer HEAD.
