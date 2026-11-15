<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Feedback filter child custody — 2026-11-14

Decision: migrate both live-mode log-filter self-reexec parents in
`codex-feedback` to the existing bounded child runner. Qualified source
`f2ad8dd74813e4eeec4da1a628f20a8220a93442`, based on
`1702dbe1df5462ad475930245615d91fc7bf15ff`, passed recorded focused tests and
scoped all-target lint in execution `22374`, exit zero. The former unbounded
output capture and loose `1 passed` check are test-harness custody gaps; this
migration claims no new RED or production defect.

The owned `feedback_log_filter.rs` parent registers `test_child_process.rs`
under `cfg(test)`, sharing the collector between its inline feedback test and
sibling `state_log_filter_tests.rs`. Each parent creates an unpaused
current-thread runtime only after selecting its CHILD-absent branch, directly
awaits the default unmarked `run(command, 30s)` entry, then applies
`Report::assert_test_success`. The runner retains its additional five-second
bounded kill/reap/drain cleanup. No outer timeout cancels that cleanup.

Acceptance requires successful OS exit, a recognized successful libtest summary
with exactly one passed and zero failed/ignored/measured tests, and error-free
real EOF on both stdout and stderr. Output retention is bounded independently
of footer scanning. The children have no completion marker; the migration adds
neither a marker nor `--nocapture`. The ordinary libtest footer remains visible
while successful child output is captured by the harness.

Both tests remain plain synchronous tests. Their presence guards, module-derived
child names, ordered argv `[--exact, name]`, `current_exe`, CHILD value `1` and
return-before-active-child structure are unchanged. Each launch inherits the
existing environment and changes only its own CHILD variable. This preserves
the fixture contract and establishes no environment isolation. Working-directory
inheritance is preserved, and null stdin matches the former `Command::output`
default. No test-thread flag is added to either child command.

The active child bodies are byte-identical, including activation, the
`[true, false]` mode order, callsites, subscriber lifetimes, captured feedback
assertions and exact state-event vector. Both children are synchronous and own
no runtime, paused clock, temporary directory, database or server. The
30-second ceiling adds a finite custody bound; no existing child-owned semantic
timeout needs headroom.

The change stays in two ALab-owned files (+28/-24). No upstream hook,
dependency, lockfile, compiler pin or shared collector algorithm changed.
Normal standalone Feedback dependencies already supply the collector's Tokio
process, I/O, macro, runtime and timer requirements without another manifest
hook. Independent static review accepted both adapters and module visibility.

## Actual validation

The source-selected Trust 15698 explicit unverified runtime-test lane ran
`unit_tests --record-results` for `-p codex-feedback --lib`, filter
`alab_live_feedback::`, with `--test-threads=1`. The recorded native unit runner
was selected through artifact identity; Feedback required no Core-specific V8
or stdio/code-mode runtime helpers.

Each of these exact outer names passed once:

- `alab_live_feedback::tests::permanently_excluded_sites_keep_their_never_interest`
- `alab_live_feedback::tests::installed_feedback_filter_follows_live_mode_at_the_same_callsite`
- `alab_live_feedback::state_tests::permanently_excluded_state_sites_keep_their_never_interest`
- `alab_live_feedback::state_tests::installed_state_filter_follows_live_mode_at_the_same_callsites`

The summary reports 4 passed, zero failed/ignored/measured, 43 filtered out and
0.02 seconds. One recorded library establishes artifact attribution; the four
exact-name pass lines and parsed summary establish the selected test result.
The two self-reexec children use their parent's exact name and are not extra
registered guard tests.

Scoped Feedback all-target Tippy completed successfully under its separate
canonical `target/tippy` target. Independent receipt checks found 1,086 unique,
existing canonical compiler artifact paths and Feedback artifacts in both
normal and test profiles. The classifier's `owned`, `spanning`, `failing` and
`scripts` lists are empty, with `set_aside=0`. Controller, unit and lint exits
are all zero. Feature inventory passed with 88 hook topics and 14 features.
The shared algorithm is unchanged; older consumer test receipts retain their
original source attribution and were not repeated for this adapter-only scope.

The complete private archive is retained at
`.git/alab-validation/f2ad8dd74813e4eeec4da1a628f20a8220a93442/feedback-filter-custody/`,
including `record.json`, `archive-index.json` and `recorded-run/`. All 24 indexed
artifact lengths and hashes were independently checked; `recorded-run/` contains
17 regular files. Raw test/native receipts and contained environment details
remain private. Only safe counts and hashes are recorded here.

| Artifact | SHA-256 |
| --- | --- |
| `record.json` | `aef3cfcb85b9b49a27c08019c2e414b4d9db8c75d076ef25c7a45bc997479085` |
| `source.diff` | `6cbf970e9a0bc8d67b92fb7157728075cb1ccba6841bdf5f6360532268c3ba85` |

Runtime and lint results qualify this fixture migration. Application
certification, packaging, installation and native accepted-actor qualification
remain separate; the existing application macro-admission gate block is
unchanged. Publication identity and receipt remain in the private validation
ledger.
