<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve the requested Python launcher and compare its runtime identity

The scheduled-update fixture fails with the managed Python 3.14 entry: it is
an executable launcher, while `sys.executable` names the package-store runtime.
Resolving the launcher's path does not identify that runtime. Probe the exact
requested entry with `-I`, then compare the observed builder runtime with that
result. Separately assert that the job preserves the exact `ALAB_PYTHON` entry.

Add an owned launcher with spaces that execs the selected interpreter, so this
case remains covered even where the installed Python is a direct executable or
symlink. Preserve the existing cache-miss, source, selection, lock and version
checks. No production behavior or compiler policy changes.

The broad initial fixture run used Apple Python 3.9, which cannot run upstream's
Python 3.11+ packaging APIs. Keep that evidence and rerun affected fixtures with
installed managed Python 3.14. The standalone native adapter remains unbuilt:
its exact Trust pin is now installed beside the unchanged default, but the
workspace's V8 submodule dependency graph is incomplete. Do not call local
Python fixtures a qualified native gate or a release result.

Validation: the new owned launcher regression fails under the old path assertion.
With the runtime probe restored, all 38 tests pass under Python 3.14. Both
explicit-builder controls also pass with a Python 3.9 controller and a Python
3.14 builder. An extra full legacy-module run retains one unrelated disk-fixture
failure: that fixture deliberately constructs its own legacy builder. No
whole-module legacy compatibility claim is made. Production files are unchanged.

## Standalone adapter follow-up

The later [native unit adapter follow-up](native-unit-runner-followup-2026-11-07.md)
records a successful pinned-compiler component build and 58 passing canonical
tests on `cf88043be53c73afc7a5944afae45fb67f6bb0c3`. This supersedes the initial
unbuilt-adapter status above. Full application/project and release qualification
remain outside that component result.
