<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Preserve concurrent main and the prune evidence

Decision: integrate current main rather than replace concurrent work. Merge
`6b35704b09e4b8908127ebf463d6fc68fba471af` preserves observed main
`849c66454c8ab9e11f234db19e006098795420e4` and the two independent topics:
`dbda07be06` defers package deletion for malformed selection markers;
`3dabbf58bd` admits the retention fixture's exact scratch for reading while
retaining its unrelated HOME read and Git write denial controls.

The final prune/promotion/retention run passed all 68 unique methods with no
failures, errors or skips. Its actual tested HEAD was `4a12d0b8`, with final
topic bytes present. The integration changes only `test_daily.py` among its
240 Python source hashes. All selected 68 inputs and both recorded controllers
remain identical, and the Python path set is unchanged. Its historical failed
run remains failed; neither result is relabelled as a new execution.

The incoming daily runner forwards explicit publisher and external target
settings into its launchd plist. Its affected tests exercise XML round trips,
absence of helper execution and expected owned cleanup targets. A separate
actual run at `6b35704b` passed all 14 unique daily tests,
exit 0 in 21.561 seconds, with no failures, errors, skips, expected failures or
unexpected successes. HEAD, all 240 Python hashes and five additional recorded
inputs remained unchanged. Evidence: `~/.codex/.ubd/daily-integration-y_mi2tr6/`;
log SHA-256 `b80fd9ac52413ab620e2749382328f6e61cf87ac31b282f0613b0d88328211aa`.
An independent hostile source review found no concrete prune blocker.
The daily review identified a separate coverage limitation: its cleanup helper
filters returned removals to known owned targets, so the new external-target
assertion alone cannot detect an extra removal request. Production cleanup
does not use `CARGO_TARGET_DIR` for deletion. Strengthen that independent test
oracle next; the passing count does not establish that negative property.

Retire only the explicitly recorded owned test worlds after verifying every
archive entry's type, mode, content hash and symlink target, unchanged source
manifests, root identities and absence of open holders. The prune evidence
retains 57 fixture worlds, two empty baseline roots and the strict original
source export in two verified archives. Their SHA-256 values are
`7a533f3f984ca62c84c92a3eb561250ca41c383c6c2f95a992e6969031b1d344`
and `b577d118cd17f325d969d069392c942481871b841735fceebb25ff298b4365a1`.
The separate two standalone retention worlds remain in an archive with SHA-256
`9afd6ffdd81753d695e831478d3a0e5ef11d7e6d61d7a787e4554b111488f3ef`.
Raw logs, failures, results and source bookends remain outside the retired
directories. These logical removals do not establish exclusive physical
reclamation or sufficient native build capacity.

This work qualifies the bounded Python fixtures, not the application. A fresh
signed updater still selects codex build `2026111701` at index 162. Original
host custody remains unavailable (native status 75), and the qualified publisher
is absent. Remote native qualification attempts refused before creating any
session; no access, identity or account pool was changed. No Rust build, native
application gate, package or actual earned reset ran. Preserve the current
rolling Sol-family/Ultra source policy, the local explicit Sol 6.1/Ultra setting,
and the unknown-owner partial primary checkout.
