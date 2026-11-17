<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Reset delivery gate entry recheck

Decision: retry the maintained application gate on a clean isolated checkout,
record the actual current entry frontier, and continue available source work.
Keep application qualification and installed adoption separate from the selected
Python controls used to develop the reset smoke tooling.

On exact source `cfa21acd9b846f0bc1e83aa280ebcc23e18f4ac9`, run:

```sh
CARGO_TARGET_DIR=~/.codex/.ubd/native-reset-gate-recheck-20261116-target /bin/bash alab/check.sh
```

The entry exits 1 before compilation with:

```text
alab: a qualified atpkg-publish is required (ALAB_ATPKG_PUBLISH)
```

The requested fresh target was not created. No Rust tests, application build,
certificate or full native-gated Python workflow result follows from this
attempt. The retained log is
`~/.codex/.ubd/native-reset-gate-recheck-20261116.log`.

An independent bounded exact-name search found no existing development frontend
in the three local aterm checkouts, their cache/package locations, `.ubd`,
`.codex/.ubd` or Codex worktrees. PATH lacks it. The executable path recorded in
the earlier fleet-mail review no longer exists; a historical controller hash
does not establish a current executable. The signed index does not name this
development tool. The README's supported provisioning route is an owned,
normally admitted producer build. No such build was attempted during this
recheck. The separate CPU observation still reports an unreadable pool due to
a slot-file identity change; it is not the gate entry error above.

A subsequent ordinary admission probe, `build-slot --wait-timeout 10 --
/usr/bin/true`, exits 2 before launching its payload. Native CPU accounting
refuses slot 0: expected device 16777231/inode 5262350, observed device
16777230/inode 5262350, requiring machine authority recovery. The retained
`~/.codex/.ubd/reset-frontend-admission-probe-20261116.log` records that separate
frontier. Thus normal admission for development-tool provisioning is also
unavailable; no compiler or producer build was attempted. No pool file or
authority setting was changed.

Preserve native authentication, the protected profile, source-selected Trust and
dependency-inclusive certification. Do not substitute exported authority,
another profile, an unverified application build, a compiler override or a
development-channel link. Earlier proc-macro and build-script certification
refusals retain their historical attribution; neither was reached in this run.

`aterm pkg update codex` separately reports build 2026111601 already current at
index 155. There is no newly adopted binary or restart from this pass. Parsing
the local settings and source defaults confirms `gpt-6.1-sol` and `ultra`; the
aterm defaults overlay contains neither override. The original remaining Core
reset wrapper and notice-delivery tests, complete gate, certified package and
installed reset behavior remain unqualified by this recheck.

## Read-only authority follow-up

The subsequent installed resource help and command surface expose only
capabilities, CPU observation and registration. The current CPU query still
reports `CPU slot file identity changed`. Available registration support is
not available capacity or permission to launch work.

A bounded source review found that the native resource host opens the canonical
domain without migration or repair. Existing-pool observation refuses a slot
device/inode mismatch. Recovery authority restores prior secret-bound settlement;
it does not grant new launch permission or rebind pool identity. The inactive
canonical-retirement path is not a client repair operation for an activated
pool. No supported recovery command was found. The unrelated installed
`aos repair` repairs the hosted cursor glow.

This follow-up observed help, source and the refusal only. It performed no pool
replacement, permission change, repair, namespace override, registration trick,
build or remote operation. Retain the native prerequisite as unavailable and
continue work that can proceed within existing authority.
