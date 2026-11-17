<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->

# Stable captured feature logs

Decision: capture declared feature-log bytes through the existing standalone
descriptor snapshot reader, then hash and parse that same returned byte string.
The implementation base is published
`302f03f46b35569d0dd7ab5977451668a56a2d72`.

## Evidence

The initial private audit supplied explicit synthetic declaration/expected
records to public `log_bytes`, `check` and `validate`; it executed no test
command or complete feature admission. Its `SOURCE=f31682d...` was a fixture
declaration, not a claim that the whole shared checkout stayed frozen there.
The loaded feature module and its direct parser/intake dependencies are Git
identical from `f31682dda8f0f126107df3ac62f115d6d30adb0c` through the base above;
the feature-module blob at both ends is
`c2c76d2f2be09f41ed332c391749dee72aabdb2c`.

Nine instrumented reads demonstrated the issue. Six same-size physical
rewrites covered all three public functions for absolute and bundled paths.
Complete image A had the requested passed-name prefix but a failed suite;
complete B had a successful summary but the wrong test name. Four own-hash
complete-image grammar controls refused those files. During the controlled
read, A's first 8 KiB and B's remainder produced a passing capture whose hash
matched neither complete file; `validate` returned no errors. Real file size
stayed 8,291 while modification/change metadata differed. Four additional
stable healthy calls confirmed the grammar control.

Three finite-growth reads consumed 57,443 bytes from an initially 8,291-byte
file after six physical appends. Those bytes equaled the final complete file,
so this establishes the missing initial-size bound, not another semantic
false-verdict claim. The private JSON has 13 rows: nine instrumented reads and
four complete-image refusals. All instrumented handles were closed with EBADF
before owned fallback; forbidden actor counters were independently uncalled.
Artifacts remain under `.codex/.ubd/feature-evidence-read-audit-20261116`.

The two maintained rewrite/growth regression methods were also run against
the exact old production module from the implementation base: 12 failed
subtest assertions, zero errors or skips. This is separate from the initial
nine-read audit. Neither result demonstrates authenticated execution, semantic
scenario coverage or native qualification.

## Interface and limits

An unbundled absolute path is explicitly resolved before capture, preserving
external files, final symlinks and parent aliases. Only that new resolve is
guarded: older pathlib's loop `RuntimeError` becomes `ValueError` with its exact
cause so validation collects a refusal. Other failures and cancellations keep
their existing authority. Bundle path, containment and leaf/log-directory
symlink checks remain unchanged; descriptor opening also refuses a final-leaf
swap after those checks.

Regular-only logs are an intentional new restriction. The old reader could
accept a preloaded FIFO; completed captured-command artifacts now require a
finite stable regular file. No FIFO fallback or weaker open flags are added.
The shared owner bounds reads by initial size plus one, checks its six stable
metadata fields, closes its descriptor and refuses missing native no-follow or
nonblocking capability. There is no new absolute cap, schema, command status,
receipt, age/expiry rule or required field.

No text wrapper is introduced. Libtest retains strict UTF-8 decoding, while
Python-result logs retain `json.loads(raw_bytes)`, including its existing BOM
and UTF-16/UTF-32 behavior. The hash is computed over those exact same bytes.
The new exact-sibling helper edge is already included in isolated feature-gate
fixtures and the release gate's existing helper pin.

These are opened-descriptor snapshots. Resolution and capture are not atomic
with an original alias or its parents; the change does not provide pathname
continuity, parent-directory custody, an executed-image proof or authenticated
test status. Existing declaration and full-native-gate limits still apply.

## Validation

Managed Python 3.14 passes 91 unique selected methods with zero failures,
errors or skips. Standard loader patterns are `test_feature_log_snapshot.py`
(11 new methods), `test_feature_evidence.py`, `test_feature_gate.py`,
`test_feature_bundle_custody.py`, `test_feature_lifecycle_contract.py`,
`test_feature_scenario_contract.py`, `test_feature_snapshot_ids.py` and
`test_focused_results.py`. The selected IDs are checked for uniqueness.
The new controls include actual rewrites/growth and FD retirement, valid
aliases/export, bundle exclusions and final-leaf swap, raw format behavior,
large regular logs, finite FIFO refusal and capability/resolve failures.

Existing component fixtures use local temporary Git and finite Python/shell
commands; the new physical-read controls launch no actors. No actual native
application, compiler, guardian, remote operation or raw PID signal ran.
The feature tool requires Python 3.11's TOML support; no Python 3.9 shim or
unsupported-runtime pass is claimed. Older resolve behavior is a controlled
exception regression, not an executed Python 3.11 claim.

Root review and independent reviews by `oracle_review` and
`reset_hostile_followup` approved all three topic paths. Each independent
reviewer passed the same 91 unique selected methods on managed Python 3.14,
with zero failures, errors or skips, and checked unchanged frozen file hashes
before and after. These overlapping selections are not additive evidence.

## Composed validation

Root tested clean committed source
`a653082da4234468d69499298b42f30ae3aa8c68` on managed Python 3.14 under
`-I -B`. All 721 selected methods passed in 94.481 seconds with zero failures,
errors or skips. HEAD bookends matched and the worktree remained clean. The
selection covers the previous 615-method component scope, all eight feature
patterns above, and the complete 18-method upstream-admission module in place
of its three formerly selected methods. It retains the prior exclusions of
the actual self-SIGTERM method and four capacity-process guardian controls.
The private log is `~/.codex/.ubd/feature-log-composition-tests-20261116.log`.

Separate discovery at that exact source found 2,113 unique IDs and zero import
errors; no test bodies or native prerequisites ran during discovery. Fresh main
`38ecb45641fd91023dd07628dc645a130b6456e0` contributes only an intake JSON and
its review note. Both are preserved, and selected Python inputs are unchanged.
These component/discovery results do not claim a complete Python workflow,
application certificate, package or installed adoption. The native resource
authority and qualified frontend prerequisites remain unavailable here.
