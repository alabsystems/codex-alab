<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Keep deferred history bound to its original writer

The first deferred-history component observed the current registration under its
lifecycle lock. An old LiveThread could follow a replacement writer after discard
and recreation of the same logical ID. Logical ID and history mode do not identify
an original writer incarnation.

Local writer installation now allocates an opaque Arc identity and returns that
same marker through the original create/resume operation while its lifecycle guard
is held. LiveThread retains the acquisition result; it never looks up a token after
an await. Snapshot reads compare allocation identity under that same lifecycle
lock before queuing the actual writer observation. Old handles and clones reject
a replacement even if ID and history mode match.

The marker owns no recorder, channel, file, file lock or cleanup task. Original
public create/resume results and preparation behavior stay intact. Unsupported
stores execute their original acquisition once and return no snapshot support.
A marker grants neither Core Current nor Native permission or settlement proof.

## Validation

Validation used source-selected Trust 15698 in the existing protected profile,
ordinary unverified test lane and canonical open-file capacity of 65536. The
combined base was 7b3ad73309efcf0417c7535e7fadbe305afd1ec9. Final 45-file input
SHA-256 is 9a917cda9e2303b9ecd3c0b154c7d5f0f426cab3d8ccc28afc3cdb44c69b7fa8.
Full library suites pass 300 store, 59 client and 613 app-server tests: 972 distinct
package/test cases, zero failures or ignores. The store code did not change after
its passing stage. Runtime job 37230 used input 42ef70d03631f8c7feea2234493db9a6acaf4db1097ccc85c51473b2ae8d68d9;
the only subsequent code change is an equivalent method reference on one ALAB
hook line. Final four-package production/test Tippy job 4813 recompiles that line
and exits zero: owned/spanning/failing/missing-script diagnostics are all zero,
with seven upstream diagnostics set aside. Dependency and compiler locks did not
change. Root checked every code/log hash, full-suite count and eleven named critical
codec/publication/generation controls. Earlier compile/lint failures are preserved.
Evidence is outside Source in codex-live-update-evidence/resume-generation-final-validation.json
and its exact input manifests/logs. The committed code blobs are checked against
that manifest. These are component and library results, not an application
certificate, accepted compiler adoption, actual Native Go or SDK/UI update proof.

The actual retained frontier/UI consumer and certified application remain
unfinished. Ordinary activation stays off.
