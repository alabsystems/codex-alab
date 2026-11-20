<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Observe every cleanup request in the external-target regression

The external-target regression's cleanup helper returned only removal requests
matching its known owned targets. An extra request against `CARGO_TARGET_DIR`
was therefore invisible to its assertion. Production cleanup does not use that
variable for deletion; this is a test oracle correction, not a production bug.

Decision: retain the complete decoded recording-rm argument lists and compare
their multiset with the exact owned removal requests after each reap and tidy
phase. Unexpected targets, arguments and duplicate requests now fail. The
filtered helper return remains for the other existing tests. An owned external
sentinel remains intact; the existing rm helper records requests without
deleting anything.

At HEAD `cd2deb8ceec6adfc5c936564b639819132ec50f9`, a fixture-only copy of
`daily.sh` was changed to request one extra external removal in each public
cleanup branch. The unchanged test passed despite five raw requests per phase.
With only the corrected test source, the identical mutant reported two assertion
failures, one per phase, and no errors. Both mutant controllers had SHA-256
`847fdf9eeac22eefb6b8e54e43197d9bbafc82306dc39136a6a44803f1a4953d`.
The production controller was unchanged throughout.

A separate healthy run passed all 14 unique daily methods in 19.848 seconds,
with no failures, errors, skips, expected failures or unexpected successes.
Actual verbose OK identities match the selected identities in order. Each
run's HEAD, all 240 Python hashes and four controller hashes matched before and
after execution; only `test_daily.py` differs between baseline and candidate.
Root independently verified the raw counterexample requests, matching mutant
hashes, final log identities and all current source hashes. Another source
review approved the narrow oracle design.

Evidence is `~/.codex/.ubd/daily-external-cleanup-oracle-f4mm30k_/`, including
both counterfactual results, the healthy result, source bookends and root review.
Healthy log SHA-256:
`147abdcbe91a4d296ae549ef6e6f0a026e07853f31fa19903e2f80f001041b50`.
Only owned Git/text fixtures, the shell and inert recording helpers ran. No
actual external target deletion, Rust build or native application qualification
is claimed. Retain logs and verified fixture archives when retiring those
explicitly owned worlds.
