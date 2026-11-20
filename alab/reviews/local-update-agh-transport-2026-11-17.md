<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Use the admitted transport for local-channel polling

At `13fa0e4db88c0299d52cb78543e5b6909af48246`, a public updater invocation
with a valid owned channel and GitHub origin attempted raw Git transport and
never called its available Agh client. The same-origin maintained transport
control used Agh successfully. Recording fronts refused network before execution;
no actual GitHub request was made by these fixtures.

Decision: lazily reuse `github.py.fetch` after channel admission, only for the
bootstrap's fetch. Adapt its `Refused` error into the existing updater failure
journal. Keep exact commit resolution, fixed-source handoff, selection guards,
and the absence of any raw-network fallback. Local bare origins remain admitted
without Agh. Preserve explicit `ALAB_AGH` in the existing LaunchAgent environment
whitelist; printing the plist does not run that client. The job fixture now copies
the shared helper alongside its existing dependencies.

One early source-only inference was disproved: raw Git also refuses a divergent
remote when the fixture restores its prior tracking observation after its own
force-push. The stronger test verifies existing refusal and unchanged tracking,
selection and build state. This is not a newly introduced refusal policy.

With identical final test bytes and the same TMPDIR, six selected regression
methods against a strict original-source export reported five assertion failures:
GitHub routing, three client-refusal subcases, and the scheduled-client setting.
The local-bare, invalid-channel and divergent-ref controls passed. Against the
candidate, all six methods passed in 5.999 seconds with no failures, errors or
skips. Public recordings verify exact-source Agh transfer from stale tracking,
no raw Git network attempt, missing/relative/refused client failure journals,
unchanged selection and no build, lazy admission, and plist client propagation.
All 1,075 original exported files were independently checked against committed
Git objects; exactly two declared test overlays match the final working files.

The broader run is **failed**, not green: 105 unique methods ran in 234.772
seconds; 87 passed and 18 parents failed with 24 assertions, no errors or skips.
One new assertion incorrectly required `Agh` in a diagnostic containing
`ALAB_AGH`; correcting only that assertion's case handling produced the final
six-method pass above. The historical run and initial baselines remain unchanged.
The other 17 controller cases could not reach their intended bodies below the
existing 20 GiB disk floor. Retained journals directly establish this for the
three writer-start hang detectors and both abnormal-handoff subcases; their
termination callbacks and custody assertions were not exercised. One strict
original-source abnormal-handoff control, run once with the broader run's exact
TMPDIR and final test overlays, failed at the same floor before its mocked body.
Native capacity recovery freed zero bytes. No floor, profile or resource policy
was changed, and the broader run was not repeated.

Publish the independently verified routing fix while explicitly retaining those
17 unverified controller behaviors for a host with sufficient capacity. This is
not full Python-workflow, feature-gate, native-build or release qualification.
HEAD and all 398 Python plus 12 shell input hashes matched within every phase;
only the test assertion changed after the broader run. Independent reviews checked
the narrow source change, actual log identities, public recordings and resource
classification. Owned fixtures and original-source bytes remain reviewable in
verified archives when their explicitly recorded directories are retired.

Evidence: `~/.codex/.ubd/local-update-agh-fix-nlt42ezo/`, including
`final-verification.json`, `root-independent-review.json`, public recordings and
native recovery. Full failed-run log SHA-256:
`ac6dff25df71a7400692e3fd7ff5d232dbca561a2bba6d3766742b993330a46d`.
Final focused-pass log SHA-256:
`e121b4339b5d293b3b607cf093cfd1cb60e915c68dcb206d16506faa85d14807`.
These are owned Git/Python/kernel fixtures with inert package producers and
selectors, not application execution or artifact qualification.
