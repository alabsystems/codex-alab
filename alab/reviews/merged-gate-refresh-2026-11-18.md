<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Canonical widest-gate refresh

The two new retained-work format warnings are fixed and measured as removed.
The post-fix canonical gate still failed at full Tippy: 954 source warnings,
0 source errors, and 2 separate locationless tool messages. Gate, typed guardian
and qualification statuses were all 1, with `qualified_source: null`.
Certification, application runtime and the later ALab/hooked-library test stages
were not reached. These results do not establish a full-gate pass.

| Source | Actual result | Boundary |
|---|---|---|
| `0daa4060` | Status 1; no qualified source | `origin/main` advanced to `44b634d3`; hook comparison reported `ALAB(defaults): 86 -> 82`. Source/tree stayed fixed and clean, but qualification recorded a changed base. |
| `44b634d3` | Full Tippy: 956 source warnings, 0 errors | 951 dead-code warnings, 3 unused imports, and 2 new format-argument warnings. Before/after qualification snapshots matched and were clean. |
| `3c6c99ec` | Full Tippy: 954 source warnings, 0 errors | Exactly those 2 format warnings removed, 0 identities added; all remaining warning identities and multiplicities equal the historical `5d80` baseline. Source/tree stayed fixed and clean while qualification observed `origin/main` advance from `44b634d3` to `9a4b37c6`. |

Commit `3c6c99ec8e8419648aa6971815d3e4d66ea95710` changes only two ALab-owned
test diagnostic expressions, in `retained_provider_work_tests.rs` and
`retained_feature_save_work_tests.rs`, to inline `{original_request:?}`. The
same Debug value, text and error paths are retained. Production, ownership,
queue/generation policies, assertions, deadlines, resources and test selection
are unchanged; no mirrored unit run was added for these two format-only edits.

Both the `44b` and post-fix `3c6` runs used the maintained
`ALAB_CHECK_BASE=44b634d3...` input to keep hook comparison stable against
the fresh main selected after the observed race. This is
a supported comparison baseline, not a hook-loss waiver: no
`ALAB_ALLOW_HOOK_LOSS` exception, intake/proof bypass or profile/compiler override
was used. Qualification independently recorded live `origin/main`; it caught
the advance to `9a4b37c6` rather than treating the whole before/after context as
unchanged. The actual Tippy failure and lack of qualification remain preserved.

After these checks, a normal merge of main
`ca4dd0ada33a46e1e000dd1dd60e6bc9163b05ce` produced
`e72f85fd5a3e7c711ab2a1037af06f1c669bce19`.
The two tested fix files remained unchanged. The gate observations above belong
to their exact recorded sources; they do not newly qualify this merged tree.

The remaining 951 dead-code and 3 unused-import diagnostics represent unfinished
integration. The concrete whole-App seam remains: lifecycle callbacks now retain
the same actor across backend rotations and refuse a leave ahead of observed
actor work; receiver/editor hooks bind actual primary/backend/actor and completed
layout before Native activation. Upgrade capture still receives App/Tui input
and editor state without consuming actual Session/Backends work cuts, while
whole-App transcript/backend capture and `EncodeInputs` assembly remain test
callers. A genuine consumer must bind the same App, all backend owners and
queue/generation cuts with the event pump alive. Empty queues, an editor paint,
a default App or serialized JoinHandles cannot substitute for that ownership.
The potentially unbounded Native settlement loop keeps its separate continuation
owner. The popup-fetch proposal remains external and unapplied.

Closed execution records:

- Earlier `44b` original `78258`, start `23a264`, terminal `35b4d6`, exit status 1.
- Post-fix original `58550`, start `a26a35`, terminal `48aadb`, actual status 1; `2026-11-18 10:34:54.966365` to `10:37:45.557819 UTC`.
- Post qualification source `3c6c99ec8e8419648aa6971815d3e4d66ea95710`, tree `521af022489b441192154d2ab89268ea73948144`; both clean, base advance recorded.
- `.ubd/codex-next-widest-post-3c6c99ec-9tplcxkr.noindex/qualification/gate.log` SHA256 `fe1448fc098397461064946c199baf4650c87701f13b7d5399c2ac256aa260e4`.
- Same root `lint.audit.json` SHA256 `d99f680f8d1d64ee769d32ac59b62a2da37f0ac4ef587c6c3f48fe749acb4aa3`; `delta-from-44b.json` SHA256 `a18f9de3e2845878b2ecb60a6854f3fddefda005d13520df84f238c32e37c351`.
- Earlier `44b` qualification/log/inventory and `delta-from-5d80.json` remain preserved under `.ubd/codex-next-widest-44b634d3-7chzbu_s.noindex`; the actor-consumer survey remains `.ubd/codex-current-actor-consumer-survey-g6jiy6gu.md`.
