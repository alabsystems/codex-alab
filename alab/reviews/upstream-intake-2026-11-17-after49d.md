<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Source-only intake after 49d

Fixed proposal/code comparison: published
`bef9ffc4faaf418899e21c838b9581be11e9f3c4`, tree
`3b0293479e710df3d601ab2cb01d4a2a38061b07`. The exact next four single-parent
commits are `49d657aad0e878fb7ab156b91126828c21a0005f..595f497f15a3e3239a62f6b99cf61067df00f0d0`:
189,935 bytes / 3,309 canonical lines. Proposal: **3 adopt, 1 superseded**.
Only this review and its intake JSON are added; source import, compilation,
runtime tests, helper preparation, defaults and publication remain separate.

Primary evidence is the canonical raw index SHA-256
`6a293abd577f28a5677daa060d9a722bf2a511dd0eaaf87e3fa0d8bb7f35122f`,
root reading `abdeefcee0dc58422d57ede92eebe52c4ca2c7d16690546adc4d58850ad86dd2`,
and independent source packet
`dfc8cc48f9c01f5fb2429f4d056ce9ef4b86ffc97ec65145caf8082646534e8d`.
The latter binds 43 Git sources, actual controls, manifests and module edges.
Root source-control review
`11ffe870d02b5df0e7eb9d4975c08712cb35dbb7e56121aeb767c81debad1da6`
also binds the eight declarations and nine module frontiers. The additive
body-span packet is
`ce37fcf95996965f2efe9f491bf896bdc7be1d913f519b5caffb7e1d5b343e95`.
Root/C whole-read canonical patches 1, 3 and 4. For patch 2 they read the
meaningful Rust/new control and bounded catalog implementation; 63 snapshots
were mechanically verified as hash-only, and the other snapshot adds an
11-line context/window block. Repeated hash snapshot text is not claimed as
whole manual reading or executed coverage.

## 1. Centralize network and shell credential setup — adopt

`05ae1b0fdd5ab412d24248acc76c5d9f6f275440` retains managed-network enforcement
when executor proxy launch settings are missing. Only explicit disabled proxy
settings suppress that enforcement. Preserve the original enforcement/live
proxy/environment tuple carried by `SandboxNetwork`, including the live proxy
through shell snapshot preparation.

`ShellStartupCredentials` classifies the original environment before rewrite.
Preserve the exact eligible protected zsh login snapshot, supported bash `-c`
conditions, arg0/startup/SSH-environment exclusions, and child-specific restored
unmasked fallback. A warned fallback intentionally remains unmasked; this is
not a blanket credential masking guarantee or a new network permission.

The fixed owned census has no direct caller of the changed named APIs in ALab
crates/glue or process paths. The existing `core/src/exec.rs` native-retained
child import, output dispatch and cancellation/settlement hooks overlap the
upstream file and must survive eventual composition. Preserve original child,
IO/FD/root/environment ownership and exact lease boundaries; file overlap is
not an observed semantic conflict or compile result.

Selected source declarations, all **UNRUN**:

- `codex-core --lib tools::sandboxing::tests::exec_server_env_keeps_command_native_and_carries_sandbox_context` — Existing async component control is expanded for missing versus explicitly disabled launch settings, retaining native argv/context and restricted/unsandboxed cases. It returns request data; it does not launch an executor or network transport.
- `codex-sandboxing --lib manager::tests::danger_full_access_defaults_to_no_sandbox_without_network_requirements` — Existing pure selection control expects no sandbox without managed-network requirements.
- `codex-sandboxing --lib manager::tests::danger_full_access_uses_platform_sandbox_with_network_requirements` — Existing pure selection control expects the native platform backend, or its existing None fallback. It executes no backend.
- `codex-sandboxing --lib manager::tests::linux_helper_uses_effective_managed_policy` — Existing Unix async fixture starts a local NetworkProxy listener and checks transformed Linux-helper argv across ordinary table/boolean loops; it executes no bwrap/helper process. Loops do not add declarations or macro cases.
- `codex-exec-server --test exec_process shell_snapshot_v2_remote_managed_proxy_uses_prepared_execution_context` — INHERITED existing local exec-server/WebSocket/TLS shell integration: one Unix declaration, seven literal cases before filtering, with six additional cases gated to Linux/macOS. It requires the actual current-test-binary helper dispatch, shell/curl, serial remote-exec fixture and actual local services; zsh absence and system-bwrap warning paths may skip. It covers masked/profile/fallback behavior, captured environment and final upstream headers, not new classifier unit coverage. The fixture joins its upstream/server tasks.

The library module chains are `core::tools::sandboxing::tests` and
`sandboxing::manager::tests`; `exec_process.rs` is its own integration root.
All selected declarations have no `ignore` attribute. Platform gates and
runtime prerequisites remain substantive; seven literal variants are not
seven newly introduced tests.

## 2. Expand the displayed spawn model cap — adopt

`5a4aff84319b4376658b4cac50fcd15fa976f6fa` changes the shared listing/error
cap from five to sixteen. Preserve its three display uses: schema description,
append-only context catalog and unknown-model error. The context still has a
1,000-byte rendered budget including escaping/markers, 250 description
characters and an omitted notice; it can display fewer than sixteen entries.
Supported exact lookup is not capped by this listing, and a hidden supported
model remains eligible under existing lookup rules. This is not a new model
authorization or child-dispatch authority.

Selected source declaration, **UNRUN**:

- `codex-core --test all suite::spawn_agent_description::expanded_spawn_model_catalog_and_error_preserve_the_visible_limit` — One NEW declaration, two literal `schema`/`context` cases, under the module's non-Windows gate. Local model/SSE mocks and actual TestCodex with test auth check visible supported choices, context description truncation versus full schema description, and exact unknown-model tool error. No successful child-model service call is exercised. The target is `tests/all.rs -> suite -> spawn_agent_description`; ordinary model/catalog loops are not additional cases.

The 64 snapshot files are expectations, not extra tests. Sixty-three changes
are hash-only; the remaining 11-line context/window addition was read as data.
No repeated snapshot text, test listing or runtime result is inferred from
their hashes.

## 3. Escape standalone installer PATH entries — superseded

`640b86dc879ceaaa126cdf183b417731e291f476` repairs shell metacharacter quoting
in `scripts/install`. Both changed paths are under the existing whole-path
omission in `alab/removed-paths`; ALab retains atpkg distribution and does not
restore a parallel installer or compatibility mode.

The omitted Python control
`InstallShTest.test_shell_lookup_after_adding_and_updating_profile` is one
declaration with two ordinary metacharacter-directory loop values. It checks
installer profile add/update, shell lookup and printed launch instructions.
It remains omitted and **UNRUN**, outside the selected Rust declaration/case
counts. This decision does not execute an installer or change the machine.

## 4. Remove the special empty SSH drop-in overlay — adopt

`595f497f15a3e3239a62f6b99cf61067df00f0d0` removes the special empty read-only
overlay on `/etc/ssh/ssh_config.d`. Configured filesystem policy still governs
allowed reads and denials. Preserve the actual native sandbox/root/FD and
network boundaries; this does not grant unrestricted SSH access or qualify
host-key handling.

Selected source declarations, both **UNRUN**, in `codex-linux-sandbox --lib`:

- `bwrap::tests::restricted_read_only_uses_scoped_read_roots_instead_of_erroring` — Retained Linux-gated filesystem-policy argument-generation control with temporary paths; no bwrap or SSH connection.
- `bwrap::tests::split_policy_reapplies_unreadable_carveouts_after_writable_binds` — Retained Linux-gated argv ordering/carveout control; no live namespace, SSH or host-key proof.

`src/lib.rs` gates `bwrap` to Linux; its `cfg(test)` path selects
`bwrap_tests.rs`. The deleted SSH fixture and
`ssh_masks_system_dropins_and_rejects_a_changed_host_key` provide no retained
coverage. Their former root/unshare/bwrap/ssh/sshd/setpriv/ip requirements and
skip/refusal paths do not attach to these pure argv controls.

## Coverage and retained obligations

The bounded Rust selection has **8 distinct declarations, 15 literal cases
before cfg/platform filtering, 4 packages and 5 package/target pairs**. These
are source declaration filters, not observed runtime or generated `--exact`
listings. No declarations are marked ignored. The omitted Python declaration,
ordinary loops, snapshots and deleted SSH test are counted separately.

Maintain exact original account/credential, command/environment, user-input,
history, usage/current native Goal budget, error/EOF/retained-terminal and
resource ownership. Existing request/attempt/Task/member/Empty/auth and source
authority gaps remain open. No snapshot/model name/late callback grants
permission. The held SourceV1/all-root/receiver routes and existing defaults
remain held; no helper flags, SDK, profile, compiler pin or compatibility API
are changed by these metadata decisions. Exact source integration, actual
dependency preparation, compile/Send/lint, native execution and qualification
remain separate.
