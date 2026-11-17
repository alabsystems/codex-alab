<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Full retained backend startup assembly

The Source backend now prepares the original CLI, Arg0, profile, configuration,
network/environment, cloud, auth, source ConfigBuilder, worktree, database,
logging and telemetry objects, then consumes those same objects through the
existing complete app-server factory. That factory requires the actual shared
Goal Gate before any root/dispatch publication. No default standalone runtime
or fresh frontend AuthManager substitutes for those objects. Actual native
binding remains a separate mandatory caller obligation before config/auth reads.

The foreground UI shares the original fallible engines, CLI mapping and logging
implementation. Its original fatal presentation remains at its caller. The
original daemon-selection match was restored byte-for-byte after compilation
caught its accidental removal with the metrics extraction. The worktree helper
forwards into the original fallible config engine with its original absent
fallback/pre-allocation owner. SDK-required login readers and the original
configuration-server OAuth exception retain their original policy.

The original dynamic SQLite filter and module mount now live in the ALab-owned
logging consumer. Filter body, layer order, private file policy and actual writer
guard are unchanged. Two upstream `quiet-logs` markers intentionally move out of
`startup_orchestration.rs` (count 8 to 6); no stale unused mount remains. A full
pre-landing gate must acknowledge this exact audited relocation with
`ALAB_ALLOW_HOOK_LOSS=1`, without intake, compiler or proof exceptions. Future
baselines containing the relocation need no permanent exception.

Runtime shutdown precedes the actual cloud/DB/log/telemetry resource drains. The
same boxed telemetry shutdown future remains owned across a canceled observer;
its result is cached before removal. Cleanup attempts precede primary error
return. Source cleanup results never certify Native effect/cohort settlement.
The UI snapshot codec carries bounded cloud/cwd/choice DATA, not Config, manager,
database or Native permission handles.

Staged checks ran 194 distinct controls: 6 Gate, 33 retained-host, 49 cloud,
18 startup component, 5 goal-admission, 45 original startup and 38 task controls.
The first 88 ran on `3945f6915253fc7c42817b1283a491e142608a831508e82cbf064f2e8f5c7043`;
the remaining 106 ran on `e7a1713b6f0566a5a0abc812d2ba6dcf16d05cef94ca12f677ae181d27e2138b`
after fixing nine TUI compile errors. The final combined input reran 33 host,
18 startup and 5 goal-admission controls (56 passes), plus the clean eight-package
lint. Earlier failures and actual input hashes remain recorded; no failing
stage is counted as passing.

Independent bounded review checked original inputs/live loader/source-builder,
shutdown/cancellation custody, logging and the final compile/lint fixes. Actual
Native origin, Go/frontier, UI/input restoration, full ordinary activation,
initial Resume/picker and later client binding still require integration. The
backend-only unsupported workload-identity/OSS/config-server interaction paths
must be completed before ordinary selection; the original foreground paths stay
functional. No application certificate or positive Native SDK handoff is claimed.

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
| `codex-rs/alab-core/glue/cloud_bootstrap_owner.rs` | `44738d0ff9eecdf307b434f49cc14d159762d2b81dcf815f80c705b1f903e65d` |
| `codex-rs/alab-core/glue/cloud_config_server_owner.rs` | `5d8d04fec891cf58c6db8f650c7943b1abfb4241307069799b6104e1bf99b3e5` |
| `codex-rs/alab-core/glue/managed_bootstrap_owner.rs` | `b2176f48f14b0614d1c5d947b74ae0939a3509638691f3583b05f318bdd6dc08` |
| `codex-rs/alab-core/glue/owned_local_client.rs` | `1f2c877f98fc89e5be2a9bb6ddfcfc25a2420175d9234f8c702fc97986ae9a3d` |
| `codex-rs/alab-core/glue/owned_local_runtime.rs` | `9f597a9c3a89606ac91d9ee52f5e4498d02ca4c7e457a38d932e5740b7fc2501` |
| `codex-rs/alab-core/glue/owned_local_runtime_dispatch_tests.rs` | `5eb6c4cdd0b5cd96b66f3a4b53ec2cee00427d05e956fe3d13f1fa38d29a6616` |
| `codex-rs/alab-core/glue/retained_goal_runtime.rs` | `5efe221faa425eefc09ae30da076f6e22e1b5f556809d23a85b8020924863a79` |
| `codex-rs/alab-core/glue/retained_goal_runtime_tests.rs` | `1362553b9d938c962299e935c6aa31069e1cde2a03ec4a3ab68b40a41c0729bc` |
| `codex-rs/alab-tui/glue/owned_local_backend_policy.rs` | `b60320e91b7a374cbfce040427ce93be3b0baa515463f10ae98868e1c7658cf0` |
| `codex-rs/alab-tui/glue/owned_local_backend_prepare.rs` | `6a3290cf045e83061cec60c5c006ccca8d9082982346fb7c289589c3282bd618` |
| `codex-rs/alab-tui/glue/owned_local_backend_prepare_tests.rs` | `60702c28d6edd7d56a25ca590ca1595005c0e150fa6cd31dc13da65215e4897c` |
| `codex-rs/alab-tui/glue/owned_local_bootstrap.rs` | `142f6d5f6f6489a032dc66f463b51bc45ad1cbd38b66005974218e37509f5c24` |
| `codex-rs/alab-tui/glue/owned_local_launch_policy.rs` | `51b818acec14afb5b6bc5a1fc5489a9bbaa23eac7700bee72232ca17e127fdab` |
| `codex-rs/alab-tui/glue/owned_local_logging.rs` | `dc4c3467fbbf93ea2ffad4e6d0efd7f559fde8721444699a50e1d3be1371b831` |
| `codex-rs/alab-tui/glue/owned_local_logging_tests.rs` | `4d7d5f9125d3cc5426c217c1e95c15876f526cbdf9aa515fe573cd1f7031bbdd` |
| `codex-rs/alab-tui/glue/owned_local_policy_tests.rs` | `04e474137b8c5f1dcdc5cff22182436f16f8447441036f6f7105d5b53cd8c541` |
| `codex-rs/alab-tui/glue/owned_local_ui_snapshot.rs` | `b158c799c1c30e6f6ab8a599e4ea8a902183527917a2e850c07c71c884ba76f1` |
| `codex-rs/alab-tui/glue/owned_local_ui_snapshot_tests.rs` | `2bf01f65e3df586539a992a090c40e874ccf09db35d55803766090ea18990c45` |
| `codex-rs/alab-tui/glue/owned_worktree_source_builder.rs` | `2a0b8fecdb4e6a4c4308f891b21d7e2f70808cfef6bc3d6c223bceabdb1113ee` |
| `codex-rs/alab-tui/glue/owned_worktree_source_builder_tests.rs` | `4ec0a597dd4a5e479126074a0ac8691dbe2cdffc4846cf271822cc25d7f68b5e` |
| `codex-rs/app-server/src/in_process.rs` | `65d7f16fe33a4f1779b6728f9e0285f1ddfc05443d5fcced7d12b5534c31e03d` |
| `codex-rs/app-server/src/message_processor.rs` | `6479d8f69023e88fb2025b40ed42d14581985cc12679106e68d0ffbfaec8876a` |
| `codex-rs/cloud-config/src/bundle_loader.rs` | `ae0f814fe1e216770be5a8dbd878baeeb1b1d1d07e0d6fa12c975593ce67c3cd` |
| `codex-rs/cloud-config/src/config_server.rs` | `3debcc86beadaf4df3037470360f6e7b1c131024338c375d702cbc64aa1e0088` |
| `codex-rs/cloud-config/src/lib.rs` | `9ce1fc2322fb269c8def826a836c4b1ce4ab86e3c377b9e3bf00b82d22752d9c` |
| `codex-rs/tui/src/app/startup.rs` | `7ea7e800de9f1c2a94a7e648a1f57f04847e917bf48af2d2ea858fa516ec274e` |
| `codex-rs/tui/src/lib.rs` | `e86515f07a800b91d4c7564a3ac8918751338c95dcfbd5363240f5d6199f85f7` |
| `codex-rs/tui/src/startup_orchestration.rs` | `fba27d8cb4cfd4f09eae00fa9ea5dfa41c58b5b25952b83952fb428af1c4d55f` |
| `codex-rs/tui/src/worktree_startup.rs` | `4c9f6e8463bc40c97cf57a15d9ab66017b7047b03c539070e02814423f7ede7c` |
