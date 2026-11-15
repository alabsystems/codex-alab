<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Check the newer installed compiler before considering adoption

Codex's source-selected Trust 15698 refused the certified application build
because of its in-process procedural-macro dependencies. Test whether the
newer installed Trust 16387 changes that constraint before considering a
compiler-pin change.

An isolated standalone fixture uses the same pinned Serde 1.0.228 derive
dependency and Serde JSON 1.0.149, with a single serialized struct. Run the
installed 16387 through the native `targo` entry, with a fresh managed target,
`trust build --certify --locked --offline`, and Codex's unchanged
`alab/proof-policy.toml` (dependency verification included).

The compiler returned exit 2 and explicitly refused `serde_derive` at the
no-procedural-macro TCB boundary. It emitted no authenticated coverage or
target inventory; the fallback report is not a proof measurement. The run
also reported unsealed launcher provenance, so it is not a source/package
qualification attempt.

Decision: do not change `trust.lock` or retry the full application build on
the assumption that 16387 clears the blocker. This probe confirms that the
same procedural-macro restriction still exists in the newer installed
compiler. It does not prove that lifting only that restriction would suffice
to certify the whole application.

The fixture, locked dependency list, command output, reports and invocation
manifest are retained in the common Git directory under
`alab-validation/605fd17466ec28157952e0ed5b4a533923b83a41/certify-probe-16387/`.
No Codex source, compiler selection or installed application was changed.
