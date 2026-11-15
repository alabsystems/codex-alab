<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Managed-auth child custody — 2026-11-14

Decision: migrate the managed-auth interface-off parent to the existing bounded
marked child runner. Qualified source
`aebd4d30588efa0402678c456f846fcfb97e7aae` is based on
`98f19f11c505cdd2a364b10dabb664a1722c0830`. Focused recorded tests and scoped
all-target lint passed in execution `78604`, exit zero. Independent static review accepted
the one-file adapter in `codex-rs/alab-aterm/src/managed_auth_tests.rs`
(+18/-14). The previous unbounded output capture is a test-harness custody gap;
this work claims no new RED, production defect or authentication behavior change.

The already test-only owned file locally registers the unchanged collector via
`../../alab-core/glue/test_child_process.rs`. The plain synchronous parent
preserves fallible temporary-directory creation, graph creation/write,
`current_exe()?` and its `io::Result<()>` signature. The newly introduced
unpaused current-thread runtime uses `build()?` to propagate setup I/O errors.
Its private `TempDir` stays bound across parent-runtime creation, the complete
awaited child operation and internal cleanup, the strict gate and normal/error
unwinding. The fixture graph continues to contain only the private `s-cafe`
entry pointing at an unbound fixture socket.

The exact ordered child argv remains `--exact`, module-derived
`managed_auth::tests::interface_off_child`, `--nocapture`. There is no added
test-thread flag. `env_clear()` still precedes exactly four assignments:

| Variable | Preserved value |
| --- | --- |
| `ALAB_MANAGED_AUTH_OFF_CHILD` | `1` |
| `XDG_RUNTIME_DIR` | Parent's private TempDir |
| `crate::locate::SELF_SID_ENV` (`ATERM_PARENT_SESSION_ID`) | `s-cafe` |
| `PATH` | `/usr/bin:/bin` |

No inherited variables or loader/auth settings are restored. Working-directory
inheritance is unchanged. The collector's null stdin matches the old
`Command::output` default.

The separate child retains its plain test attribute, print-site lint expectation,
presence guard and complete synchronous body byte-for-byte. It activates the
process switch, disables the interface, checks the independent managed-auth
endpoint, installs the process-local host and prints the existing completion
marker. It constructs no runtime, pauses no clock, starts no task/server and
connects to no credential or authentication service. Production managed-auth
code is unchanged. The adjacent fake-server refresh test owns its own unrelated
resources and remains unchanged.

The parent builds an unpaused current-thread runtime and directly awaits
`run_with_stdout_marker` with a 30-second internal deadline and literal
`MANAGED_AUTH_OFF_COMPLETE\n`, including an actual trailing newline. The
existing `--nocapture` exposes that print. Success requires the marker,
successful OS exit, a recognized successful summary with exactly one passed
and zero failed/ignored/measured tests, and error-free real EOF on stdout and
stderr. Retained output is bounded independently of marker/footer scanning.
The marked gate rejects a successful guard-skipped child.

The collector owns its additional five-second bounded kill/reap/drain cleanup.
No outer timeout cancels the awaited future and no child task is detached.
External cancellation retains only the collector's existing `kill_on_drop`
request; it promises no asynchronous reap. The selected synchronous child has
no semantic hang guard needing a longer parent ceiling.

No upstream hook, manifest, dependency, lockfile, compiler pin or shared
collector algorithm changed. Standalone `codex-alab-aterm` directly declares
the Tokio process, I/O, macro, runtime and timer features required by the
collector. The focused module and scoped package all-target lint cover this
adapter; unchanged older collector-consumer closures need no repeated claim.

## Actual validation

Source-selected Trust 15698 ran the explicit unverified runtime-test lane with
recorded `unit_tests` for `-p codex-alab-aterm --lib`, filter
`managed_auth::tests::`, and outer `--test-threads=1`. All five exact outer names
passed once:

- `managed_auth::tests::pending_without_known_revision_is_strictly_bound_to_the_refresh`
- `managed_auth::tests::reply_binds_every_field_and_rejects_unknown_duplicate_or_secret_errors`
- `managed_auth::tests::authenticated_refresh_retries_waiting_without_claiming_autopilot`
- `managed_auth::tests::endpoint_and_install_remain_available_when_the_interface_is_off`
- `managed_auth::tests::interface_off_child`

The parsed summary reports 5 passed, zero failed/ignored/measured, 185 filtered
out and 0.13 seconds. The final guarded child is inactive in the outer run;
the parent independently launches its active exact child and requires the
marker. One recorded library establishes artifact attribution separately from
the exact pass lines and summary. Native unit-runner identity was revalidated;
this package needed no Core-specific V8 or stdio/code-mode helpers.

Scoped `codex-alab-aterm` all-target Tippy completed under its separate canonical
`target/tippy` target. Independent stream checks confirmed 239 unique, existing
canonical compiler artifact paths, both normal and test profiles for the
aterm library, and successful build completion. The classifier's `owned`,
`spanning`, `failing` and `scripts` lists are empty, with `set_aside=0`.
Controller, unit and lint exits are zero. Feature inventory passed with 88 hook
topics and 14 features. Unchanged collector algorithms required no repeated
claim for older consumer checks.

The complete private archive is retained at
`.git/alab-validation/aebd4d30588efa0402678c456f846fcfb97e7aae/managed-auth-custody/`,
including `record.json`, `archive-index.json` and `recorded-run/`. All 25 indexed
artifact lengths and hashes were independently checked; `recorded-run/`
contains 17 regular files. Raw test/native receipts and contained environment
details remain private. Only safe counts and hashes are recorded here.

| Artifact | SHA-256 |
| --- | --- |
| `record.json` | `0314133e76af7a3da70f1be9210cdde55fe7ee8012b9ebb772dbaceef363b7cd` |
| `source.diff` | `8a9b97d93b81611702c854c71a7cba99c6a26754f5f9aee73e75928789481860` |

Runtime and lint qualify this test fixture. Application certification,
packaging, installation and native accepted-actor qualification remain separate;
the existing application macro-admission gate block is unchanged. Publication
identity and receipt belong in the private validation ledger.
