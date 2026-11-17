<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Final merged-check publication alignment

Decision: preserve concurrent main changes through a normal merge and verify the
affected fixture suite before retrying the guarded publication. The first
publication, original process 75016/completion 3d1b65, returned 1 with the
maintained publisher's explicit `main advanced before publication` result. It
did not publish the candidate. The original log and guardian receipt are retained.

Fresh Agh API and fetch observations agreed on
`9159727cb17c9f2e3c61e68768907e4bb08e096a`. Its three incoming paths are the
weekly-reset oracle, its tests and their review. The normal merge produced clean
`149b40398a3d323fe53807b780d3fe459fcbaefd`. The delta from the earlier joint API
source is empty for `codex-rs/` and `trust.lock`; no native rerun is needed for
these Python-only changes. The earlier [library results](native-library-custody-2026-11-16.md)
retain their exact source boundaries.

Original process 60062/start 8378db/completion 643cc9 runs the actual maintained
Python result CLI against `alab/smoke/weekly_reset_test.py` on that clean merged
source. All 29 distinct methods pass, with no skips, failures or errors and actual
outer/child status 0. The 855 captured regular inputs match Git and remain
unchanged, along with the three recorded executable maps. This is fixture-model,
owned-loopback and wrapper-child evidence, not native Codex/provider/reset-credit
execution or certification.

The private common-Git archive is
`alab-validation/149b40398a3d323fe53807b780d3fe459fcbaefd/weekly-merge-python-private-iobajy55`:
869 payloads, 12,564,657 bytes and 855 Git bindings; index
`93e7b5332e6304639150b1d31a3fbc9a82857380ac0db4e0d6634985641d087c`.
Its directories are 0500 and files 0400. An archival-only Python 3.9 counter
comparison failure is retained separately from the unchanged passing test run;
the same archiver succeeded under the original managed interpreter. No test was
rerun for that archival correction. Publication retries bind a fresh main lease;
remote ancestry and retirement of only this session's branches are checked
separately after a successful push.
