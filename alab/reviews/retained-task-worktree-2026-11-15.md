<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Retain original worktree policy in the backend

A replaceable frontend must not own an accepted checkout allocation. The
retained task host now consumes the original managed-worktree schemas,
WorktreeManager, source trust checks and attachment store through a narrow
ALab-owned adapter. It keeps the original argument/display bounds, local-only
checks and explicit workspace-root result; creating a checkout changes neither
the issuing task cwd nor its permissions. Only pending matching operations join
under the original policy; completed-call deduplication is not claimed.

The actual original ConfigBuilder is a required process-local input for hosts
that expose worktree tools. It passes through the existing client/runtime owner
to the processor before dispatch. A host without that resource omits the
capability instead of reconstructing config or supplying defaults. Subsequent
source-project trust checks use that same builder and its actual live loader.

The retained host tracks the whole async worker and actual blocking allocation.
It captures the exact checkout immediately after Git creates it, before binding
or fallible attachment registration. Unknown worker loss remains unknown after
late allocation and cannot be converted to success by Source cleanup. Removed
records release owned resources outside registry locks. These Source lifetimes
and cleanup results do not close Native effects.

Four actual HTTP/Git/SQLite controls pass: allocation survives HTTP observer
loss and C1/C2 replacement, a fresh explicit untrusted source is refused before
Git, an absent original builder exposes no worktree capability, and accepted
worker loss retains Unknown while actual blocking drain waits for Git. The
final checked input passes 38 task controls, two actual bootstrap controls,
12 TUI dynamic-tool tests and 31 existing retained-host tests: 83 total, with
zero failures or ignores. Production/test-library Tippy has no owned, spanning,
failed-target or missing-script findings. Three measured style diagnostics were
corrected before this final full component rerun; no lint was suppressed.

The normal source-selected contained resolver adds only the local adapter
package and app-server dependency edge. Every existing package version, source
and checksum is unchanged; no lock entry was hand edited. Component checks use
source-selected Trust15698 in the protected explicit unverified runtime lane.
They retain their actual committed-base-plus-exact-20-file-patch identity. The
subsequent commit verifies every checked code/manifest/lock blob; this is not a
clean-commit application build or certificate.
Ordinary retained preparation, real Native origin/Go, frontend/input handoff and
actual new-version qualification remain unfinished.
