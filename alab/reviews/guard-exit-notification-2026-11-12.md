<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Prefer exit notifications for disk-guarded commands

Base source: `b58c000564bd8fefa313bdbee5ed5e4108d986d7`.
Owned checkout: `~/codex-alab-guard-exit-20261112`.

## Decision and scope

Use the existing kqueue observer for `observe_exit_with_disk` whenever kqueue
exists, including Python versions that also expose `os.waitid`. The old dispatch
selected a nonblocking waitid probe followed by a one-second sleep on those
versions. A short command that exited after the probe then waited for the next
probe before its guardian could finish.

The kqueue implementation already waits for `NOTE_EXIT` without reaping and runs
the disk check before each bounded wait. Reusing it preserves the disk interval,
existing error handling, and the custodian's process-group cleanup. Blocking
`observe_exit`, the waitid fallback when kqueue is absent, and raw command result
handling are unchanged. This changes only ALab-owned Python code; no compiler,
Rust build, native admission, or retained build checkout was changed.

## Deterministic regression checks

Before changing production dispatch, this command failed on Python 3.14.5 with
`AssertionError: kqueue must observe the disk-guarded exit`:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_command_guard.py -k test_disk_observer_prefers_kqueue_and_keeps_the_original_exit_registration -v
```

The regression exercises the public disk observer with both APIs present. It
forbids waitid, models one disk interval without an exit, then supplies
the exit notification. It checks that the original registration is submitted
only once and disk checks retain their interval. Other added tests cover a real
guardian's cleanup after kqueue `EPERM`, the fallback's non-reaping flags and
disk-check order, resource and observer errors without retry or reaping, and the
unchanged blocking waitid observer. No test asserts wall-clock speed.

Final focused commands, from the owned checkout:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_command_guard.py -v
/usr/bin/python3 -I -B -m unittest discover -s alab/tests -p test_command_guard.py -v
git diff --check
```

- Python 3.14.5, with waitid and kqueue: **26 passed**, 0.528 seconds.
- System Python 3.9.6, with kqueue and no waitid: **23 passed, 3 skipped**,
  1.058 seconds. The skips are the waitid-specific cases.
- `git diff --check`: passed.

The real child-lifetime tests include exact normal and signal exit statuses,
disk failures after launch, descendants, and custodian reaping. The waitid-only
capability shape is simulated on this Mac; no Linux runtime was used.

## Short-command measurement

Retained evidence directory:
`/tmp/.ubd/codex-guard-exit-20261112-hezrd23d`.
It contains the baseline and candidate Python files, `measure.py`, `results.json`,
and the final focused test logs. The baseline file is the exact `git show
HEAD:alab/command_guard.py` at the base commit; the candidate is the edited file.
Their SHA-256 values are respectively:

```text
1caaddc161cf9d21dd14ba7f840e185566647e4a1a01901857442d9b22508091
431cb220925b537bf4f2059b1da61954dbcfda2c5679ee55c7311cfb4cf3bfad
```

The retained script runs three sequential baseline/candidate pairs with Python
3.14.5, each using the actual guardian CLI and this command tail:

```sh
--min-free-gib 1 --disk-path $HOME/codex-alab-guard-exit-20261112 -- /bin/sh -c '/bin/sleep 0.05; exit 23'
```

The equivalent reproduction command is:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B /tmp/.ubd/codex-guard-exit-20261112-hezrd23d/measure.py $HOME/codex-alab-guard-exit-20261112 /tmp/.ubd/codex-guard-exit-20261112-hezrd23d
```

| Pair | Baseline seconds | Candidate seconds | Both exit codes |
| --- | ---: | ---: | ---: |
| 1 | 1.041773 | 0.105522 | 23 |
| 2 | 1.047811 | 0.110403 | 23 |
| 3 | 1.049132 | 0.108623 | 23 |

Median elapsed time was 1.047811 seconds before and 0.108623 seconds after.
These times include interpreter startup, the 50 ms command, guardian startup,
and cleanup. `aterm ctl @self sys` before measurement reported 15.2 of 18 cores
busy, fair thermal state, normal memory pressure, and no swap; a later reading
reported 9.0 busy cores. Other work continued on the machine. These observations
demonstrate the avoided polling tail for this short command, not a quiet-machine
benchmark or an end-to-end check/build speed claim. The separate running
`CheckLintStep` baseline was left unchanged.

## Integration validation

All 39 lint-step integration tests passed in 146.410 seconds with the candidate
and the [recovered native unit runner](native-unit-runner-recovery-2026-11-12.md).
The fixture harness ran with its documented plain-Python invocation and explicit
`ALAB_TEST_UNIT_RUNNER`; individual contained cases retained their own sandbox.
An earlier run on the unchanged source had incorrectly wrapped the whole fixture
harness in another sandbox. It was stopped and is not a runtime comparison or
a product regression. All four observed failures from that invocation passed
with the correct invocation on unchanged source before this candidate ran.

Another 21 guardian integration tests passed in 3.498 seconds: post-fork cleanup,
guardian lifelines, raw-result attribution and capacity admission. No test in
either run failed, errored or skipped. These checks do not claim full native
release qualification. Independent review found no remaining issue in the
notification change.
