<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Retained backend goal admission

A retained host needs to keep fresh automatic goal work out of the replacement
window while retaining already accepted work. One actual Owner creates an
initially held Gate. Owned read guards cover the original goal read/start or
Autopilot read/submit window; a replacement Hold drains them. A queued writer
defers new admission. Release checks the exact Owner/Hold generation and caller's
actual runtime binding; withdrawal permanently closes future admission. This
scheduling primitive supplies no Native, SDK, provider or model permission.

The real ThreadManager seals registration at the common first Core start and
attaches that same Gate to every real root/child before goal publication. Both
the native GoalRuntime and Autopilot path retain the real admission guard through
the original window. The no-GoalRuntime path uses the actual thread extension.
The original same-auth-wall accepted-turn recovery path remains unchanged; this
is not a claim that every kind of accepted recovery is blocked.

Six primitive controls passed on input `3945f6915253fc7c42817b1283a491e142608a831508e82cbf064f2e8f5c7043`.
A separate existing integration fixture binds the actual manager before real
Core start, verifies the actual GoalRuntime receives that Gate, observes no real
model request while held, drains its actual automatic permit, then permits one
model request after release. All five goal-admission controls passed on `e7a1713b6f0566a5a0abc812d2ba6dcf16d05cef94ca12f677ae181d27e2138b`
and the final combined input. A fixture release callback is Source scheduling
evidence only, not a positive Native Go test. Independent review checked the
binding, permit/drop order and original accepted-turn recovery exception.

The genuine physical backend must own the one Owner/Hold before its receive
worker/config preparation, pass the same Gate to the required backend factory,
and accept/drain H1 before acknowledging Native. That receive handler and actual
R0/R1/Core/listener/UI reconciliation are not mounted by this component. Source
primitive and fixture passes do not qualify the real update path.

Validation used the source-selected Trust compiler and the existing protected
project runner. The application was not built or certified. Ordinary retained
startup and actual Native SDK/UI activation remain unselected and unqualified.
The live-update objective is incomplete.

The checked final combined input is
`4725aa93487769acb2f69df567c70aab3def3bdf2f04937abc3b236eda8447e1`
on `f56a12c11957d8218e59f403847147019d4b9641`. Eight-package production/test
Tippy found zero owned, spanning, failing or missing-script diagnostics; 16
upstream diagnostics were set aside. A manual audit of warnings in changed
upstream paths found only three duplicated original ClientRequest patterns,
unchanged from the base. Exact logs and terminal statuses are preserved in the
workspace `retained-startup-*` records. These dirty-input checks are not relabeled
as a clean-commit application build.

Checked code hashes (SHA256):

| Path | SHA256 |
| --- | --- |
| `codex-rs/alab-autopilot/glue/goal_admission.rs` | `72a4dda990821fcb9df1a54a72b5a1317669bec0cea2182b5b2b27714510bb6b` |
| `codex-rs/alab-autopilot/glue/retained_goal_admission.rs` | `e952a573f202874c2ef83d928794cbc3f97f0d9b68f2e67c5fc4004fca62e3e8` |
| `codex-rs/alab-autopilot/src/extension.rs` | `b8b702f227e1fed8eedeff3b603755a4f67358a366906ba1d328cc6c2eabd59a` |
| `codex-rs/alab-autopilot/tests/goal_admission.rs` | `dfc6914bf08ed370241c4939a8e0359d4b1a37e69b4a34eb98ab0f75b0eddbd5` |
| `codex-rs/alab-autopilot/tests/goal_admission/retained_goal_gate.rs` | `ed8ab845a509096ca5ef99388aa79c2b33c7ebd1b2ffe278cff3ce1a54a70070` |
| `codex-rs/alab-core/glue/retained_goal_manager.rs` | `5b70304d74c5d85d243bc00f06d77041cc72c4d769819e01b5b6346825e6cd5f` |
| `codex-rs/alab-core/src/lib.rs` | `0887e8c174a0895cae24155a8f82800e4ee3d07d7d7bc869b0e48c6c21e88221` |
| `codex-rs/alab-core/src/retained_goal_gate.rs` | `5e44eea5b0673226b65b276c4ef8e6df596759636325b302060ca7a005b141ea` |
| `codex-rs/alab-core/src/retained_goal_gate_tests.rs` | `3e65faea7c8da64ebafa99d93fc2a9c3fb2fa081291ca530852c630bdfa7f4ff` |
| `codex-rs/core/src/thread_manager.rs` | `ef5bc4379ea0eda59276104df194f154f92201262ddab05d8e594fceb413a1c8` |
| `codex-rs/ext/goal/src/extension.rs` | `ce2d0bf2cdd3b577c8934350f941d233f65c5564b3c9079fdf80b4e81f9009e3` |
| `codex-rs/ext/goal/src/runtime.rs` | `9d33992d1a2be7d0f763c7cf498aac4d2747fcbf094ada7c683516acd52ddf26` |
