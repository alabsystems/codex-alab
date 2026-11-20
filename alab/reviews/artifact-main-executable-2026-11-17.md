<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Running artifact identity requires the kernel main executable

At `0e35101339b8c12b7f016f2cc53088c2fd47b6e6`, the public status CLI reported
an owned inert package as running when a Python child merely mapped its
`bin/codex` with `mmap.ACCESS_READ`. `lsof -d txt` includes ordinary mapped files,
and the reporter selected any unique mapping whose basename was `codex`. The
kernel's `proc_pidpath` identified Python throughout, but the CLI returned the
fixture artifact SHA, no errors and exit zero. The file was never executed.
Without that mapping the same Python program correctly stayed unknown. Explicit
package and selected-executable observations remained independent and correct.

An owned ad hoc signed copy of `/bin/cat`, packaged as `bin/codex`, provided a
genuine main-image control. Its echo proved first execution before inspection;
kernel path, loaded vnode and package identity agreed. Both follow-up children
were released through stdin EOF and reaped with exit zero. Source, image and
manifest bytes stayed unchanged. This is a process-observation proof, not a
Codex application or release qualification.

Decision: obtain the main executable path through the typed macOS `proc_pidpath`
ABI, then require exactly one matching loaded path/device/inode tuple and the
existing package descriptor observations. Read that kernel path before and
after inspection inside the existing process-start fences. Unrelated mappings
named `codex` no longer select or disqualify the known main executable. Refused,
truncated, malformed or changed native paths leave running identity unknown.
Native PID bounds and a 4096-byte buffer follow the local SDK's `pid_t` and
`PROC_PIDPATHINFO_MAXSIZE`; system-library loading occurs only for the requested
macOS running-process observation. No argv/comm fallback is added.

The existing process-start observation is second-granular, and these checks are
not an atomic snapshot, a PID-generation lease or continuous image custody.
Packaged source remains a manifest claim; no qualification is inferred.

Four maintained public observation regressions cover foreign mappings, native
path refusal, changed main paths and a genuine main image with another `codex`
mapping. Portable fake-PID tests explicitly supply their kernel observation.
An obsolete global-basename ambiguity case is replaced by that stronger positive
control; missing mapping, device/inode, PID and package-replacement checks remain.
With identical final test bytes and a three-file Git-object-bound old-production
export, five selected parents produced five assertions across four failed
parents; the ordinary genuine control passed. The sole four-module candidate
run passed all 99 unique methods in 21.998904 seconds, with no failures, errors,
skips or expected/unexpected successes. All 415 declared Python/controller/README
inputs and three old-export hashes were stable within both phases; only the
production artifact reporter differs between phases.

A separate real public-CLI replay then reused the exact same physical inert and
signed-cat inputs. Python still mapped the inert file, but now running identity
was null and the CLI exited one. The genuine main executable still reported its
fixture SHA with exit zero. Explicit package/selection remained unchanged. Both
fresh children proved their expected behavior, exited through EOF and were
reaped; the recorded PID checks found neither remaining. Root and peer separately
verified actual ordered test results, Git blobs, source reversal, native paths,
raw outputs and physical input continuity. A read-only native check also confirmed
this session's existing PID 98846 really runs managed build 2026111701/source70d.

Evidence: `~/.codex/.ubd/artifact-main-image-audit-8t2s9rst/` and
`~/.codex/.ubd/artifact-main-image-fix-kansqh2a/`. Candidate log SHA-256:
`e206ff75b3c9239fc48a4ea47708fc6a43a5a1ee26e191068ccff0d81c96d9af`.
The exact post-fix replay runner was saved before execution, SHA-256
`c5c0b6830f0150c835bdc86435db3f034985561e7053ae8d3f07e414f40babb3`.
Only the three proof leaves and old-production export were verified and retired
into archive SHA-256
`84dbbc4f7c4dd40a6399f3f9b7019b5ccdcb49aa2008c91303cb04f9f2cb67b7`.
Raw original/failed/final evidence remains. No Rust build, live reset credit,
installed package change or native Codex qualification occurred.
