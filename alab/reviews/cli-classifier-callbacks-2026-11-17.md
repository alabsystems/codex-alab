<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Callback-safe CLI exit classification

The admitted source base was `4da443a5f0b8b29ea78c3174057e0a1e10081d31`.
The shared classifier now accepts only exact `SystemExit` objects. Every subclass,
including a transparent subclass carrying 47, raises the fixed ordinary
`RuntimeError("unsupported SystemExit type") from None` before consulting code,
notes, names or string conversion. This is an intentional restriction of the CLI
classification domain. Raw library/main bodies still raise their original objects;
plain `SystemExit(47)` and `KeyboardInterrupt` retain their identities.

The code payload's actual type is tested with `issubclass(type(code), int)`.
Unbound `int.__index__` still extracts an integer subclass's native value before
the existing POSIX bounds and mask. An opaque payload remains a non-success exit;
its `__class__` hook is never invoked by classification. None, boolean, floating,
wrapped, huge and ordinary integer behavior remains covered.

Inventory found three standalone copies: `file_hash._successful_exit`,
`smoke/capacity_exec._successful_exit`, and the early history guardian's nested
`successful_primary_exit`. All now use the same exact-type restriction and actual
payload type test. Immediate cleanup selectors use actual exception types too,
so they cannot dispatch `__class__` before entering a fixed classifier. The early
guardian retains its local non-SystemExit false result and its closed
`os`/`sys`/`primary` dependencies. No production import was added or changed.
This change does not replace advisory diagnostic renderers with a new policy.

Python 3.14 ran with `-I -B`. The initial feature CLI baseline passed 20 tests and
343 actual owned Python children in 34.276 seconds. The candidate focused run
passed 26 tests in 39.416 seconds: four classifier tests plus 22 feature CLI tests,
with 349 actual children. All 343 prior signatures matched: consumer/case/entry,
actual OS status, call/return counters, escaped type/code/identity, and output
creation fields. These are structured signatures, not an equality claim for
temporary-path-bearing traceback bytes. The original assertions on output and
source binding passed as well.

The six new feature children run the real original upstream-status entry or raw
main while replacing only report observation with an inert raising seam. They
cover the hostile code getter, hostile payload `__class__`, transparent exit
subclass, integer subclass, and both raw hostile main controls. Hostile witnesses
use actual types, the base `SystemExit.code` descriptor, primitive/opaque tags and
no foreign repr or JSON payload. The report and owner never return, stdout is
empty, and all forbidden actor counters stay zero. The entry exits 1 in each new
case; both raw hostile objects escape unchanged and actually exit 1.

A separate six-child baseline replay used the exact committed old helper loaded
into memory in the same original entry fixture. Both hostile entries actually
exited 0; raw hostile main exited 1; plain 47 exited 47 and plain 0 exited 1.
The replay took 0.689 seconds. This is a controlled helper substitution in actual
Python children, not native application, transport, admission or custody proof.
The helper's SHA-256 was
`1ae84f16f1fc83ecb8797ab71835af21152eaa60c486e3b0d229f81284e8d36c`.

After adding the public runtime-hash and output-close regressions, a final
15-module consumer run passed 127 tests, zero skips, in 33.612 seconds:
`test_cli_status_callbacks`, `test_runtime_file_hash`, `test_history_file_hash`,
`test_history_cli_status`, `test_cli_status_binding`,
`test_qualification_cli_status`, `test_request_recovery_cli_status`,
`test_reset_smoke_cli_status`, `test_smoke_cli_status`, `test_capacity_exec`,
`test_write_json_cleanup`, `test_runner_output_ownership`,
`test_runner_signal_ownership`, `test_runner_child_cleanup`, and
`test_smoke_package_records`. This includes the isolated early guardian closure
and the release-hook's committed helper binding. The four production identities
below were identical before and after that run. `git diff --check` passed.

| Source | Before SHA-256 | After SHA-256 |
| --- | --- | --- |
| `alab/cli_status.py` | `1ae84f16f1fc83ecb8797ab71835af21152eaa60c486e3b0d229f81284e8d36c` | `1c319a424044d26516e18029caa4eea786eb6292ab412937347aabe7e531e7ba` |
| `alab/file_hash.py` | `3e0ba615caf6d5321591281f6d8828345e046877552c10eec726727fd8f46748` | `0daa1dd9f4a1215f0ca3b50d92694dcf082efe3f9eb16ce02534d3d7dabbb6b5` |
| `alab/smoke/capacity_exec.py` | `707cab64a336bac14ac5cef6fbb6f15f9eaf04f600442d8af41133bcf5cba87b` | `2402689258bf67637f9d3cebbb0311a7a8ad6639b539a21b45b1410d9bee7b13` |
| `alab/smoke/history_pagination.py` | `5f2cf76b77b7477c494db07e2be8b70a806d41e547b878fa5d66a72d582889e7` | `2d60d1e80b1b50b7d7d1573dd17fc0d28142a661c00470590066526cfea3d894` |

`upstream_status.py` stayed byte-identical at
`92cb66c47dedfeb894266242f840720def0a155bd1b7408f95d0efb1168080dc`;
feature-evidence, feature-gate and upstream-gate sources also stayed byte-identical.
The independent native subagent review found no blocking issue in the shared
helper, closed copies, selectors or callback-safe witnesses. No saved report,
native qualification, certified application build, package or installed adoption
is claimed by these Python source tests.
