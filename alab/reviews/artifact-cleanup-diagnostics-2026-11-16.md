<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Expose retained artifact cleanup diagnostics in local reports

Artifact inspection retained secondary descriptor-close failures as notes on
its selected exception, but observe returned only the primary string. A real
owned-descriptor control produces two close-EIO notes; both disappeared from
the artifact report and from upstream_status.report's forwarded errors, even
though healthy selection and source observations remained available.

Decision: append existing notes immediately after their stage's unchanged
primary diagnostic, prefixing each with the same stage and `note:`. Ordinary
note-access/iteration/formatting errors retain the primary and any already read
notes, then allow later stages to proceed. KeyboardInterrupt and SystemExit
still propagate; primary formatting, error classification, identity validation,
resource ownership and report schema are unchanged. Existing consumers forward
the errors list and use its truthiness, so additional diagnostics require no
consumer change. A stage without notes keeps its exact previous output.

The same eight new maintained controls run on coherent exact
`a1b8333e6fd293d52092248c93f15cbd5bf2a38a` and fixed source with identical final
tests and helper bytes. Baseline: actual exit 1, six assertion failures across
six parent methods, two passing methods, zero errors/skips. Fixed: actual exit
0, all eight pass, zero failures/errors/skips/expected failures/unexpected
successes. Original outer driver also exits 0; source and test hashes remain
unchanged. Only artifact_status.py differs between selected source bundles.
Notes arise from real owned-descriptor close faults. The note-access fault
subclass delegates real add_note and activates only after cleanup; no notes or
observations are manufactured. Controls check primary/note order, no-note
compatibility, healthy actual selection, ordinary accessor/iterator faults,
exact reporting cancellation identity, and actual local-Git report forwarding.

Existing cleanup fixture helper methods remain byte/AST equivalent to baseline;
one public expected-errors assertion intentionally adds its naturally produced
cleanup note. Module imports/helper assignments add eight controls without
rediscovering the existing 24 cleanup tests. Independent static review approved
all frozen implementation/test identities and consumer compatibility.

Root validation runs four complete affected modules on clean committed source
`52bd19b5a47f66c3f505c213f383a179a3dde2d8`, tree
`e54f2f1d01413fe195438b795678d5093cd49810`: 77 distinct methods (18 artifact
status, 24 cleanup, eight diagnostics, 27 upstream status), actual exit 0 in
16.038s. No failures, errors, skips, expected failures or unexpected successes.
Head, tree, clean state, captured sources/scripts/tests and Git/Python/sh/bash
entry identities are unchanged before/after. The eight counterfactual methods
overlap these 77 and are not additive. Tests use local Git/package fixtures and
owned descriptors; existing process-image probes mock ps/lsof. No artifact
execution, network fetch, native gate or application certification is performed
by these tests. Production SHA-256:
`b6b9294d645341b151b93e1ab7404a9dc8717f59474aca92eabbe0af60913c37`.
New test SHA-256:
`f78704efc77d391774faa6cafa54538df083a390066dceec7f415ab6bf18ccde`.

Evidence is retained under common Git
`alab-validation/52bd19b5a47f66c3f505c213f383a179a3dde2d8/artifact-diagnostics-mfcrbs8_`:
175 payloads, 1,970,993 bytes, including coherent counterfactual source, original
proof, drivers, raw logs and complete root snapshots. Exact recursive inventory,
all sizes/hashes and selected source Git bindings were verified. Index SHA-256:
`64475433463cc78f99601df155657656c9e6c2ff8552ceb06d6b902d2a45633b`.
Final root log SHA-256:
`75f66835f85163cc9279ff4c73cd29a208469a503f674ae82e73b7fa9918ef0b`.

Main advanced during the first publication attempt; that attempt made no
write. Main `78409791d424992d2a0f5888696cfee57c4a344f` was integrated afterward.
Its review document and native API test changes leave every captured input of
the 77-method Python run identical. The result remains bound to its tested
commit and does not claim execution of those incoming native tests.
