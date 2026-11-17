<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Existing workflow fixture behavior across the CLI guard change

The [workflow CLI repair](python-workflow-cli-status-2026-11-16.md) already has
failure-path controls. This separate measurement checks whether the unchanged
existing fixture behavior differs between exact baseline
`f7bee529ee12ecca4e0f752c66c30cf4c063784e` and landed source
`610696ddc177d4725ac598295b62decd5fb3f5ce`.

Both arms run the same 33 existing PythonWorkflowTests against private immutable
three-file closures: `python_tests.py`, `cli_status.py` and
`tests/test_python_tests.py`. Only python_tests.py differs. Each child and the
original outer controller exit 0; each child passes the same 33 distinct methods
with zero failures, errors, skips, expected failures or unexpected successes.
The two repeated runs are 33 unique methods, not 66 distinct tests.

The observer calls through the existing fixture output helpers and original main
and captures 57 ordered functional observations per arm: 48 output-helper
observations and nine original-main outcomes. Complete manifest and summary
objects, semantic status descriptors, case identities/outcomes and event order
match without removing a structured field. Both canonical functional hashes are
`f47469d68c167adc8e8f657e6d382a6ac1dc63136a04eac745414d67d309c33e`.
Raw stdout, stderr and tracebacks remain separate unfiltered records; their bytes
are not claimed equal. This is a functional differential, with no performance
conclusion or speed ratio.

Source and tool identities match before/after. Baseline python_tests.py SHA-256 is
`be2d03cdc9b2c69b99a2a45c1ead2ed8dca32ae9f162d7a10e8597c69101db22`;
landed SHA-256 is
`bd9ac3ebfe061b86c5f1b4a87e9c05b4c2b7beb56d4627c11de04b8559ae0d57`.
The existing fixture SHA-256 is
`646dc7dc26d9185e5d9a4ea19b9454763a59ea2fcdbe1c9024fd6fd7f343317d`;
the shared helper SHA-256 is
`29a7e95972c02612081af204d5c16a6b5a41e575cae1c825f43c6964f2ad20c2`.
The controller SHA-256 is
`beca48a012153c395cffe4f75712a027e46703d66c0bf71ea149a784763deb82`;
the observer SHA-256 is
`23bcaff8ace56a67a2012fa81bf2947d09956f42555ac489bbe6b45c1c0444f2`.
The recorded tools are managed CPython 3.14.8 and Git 2.56.0. The controller's
working repository Head was `633f5b23ed336a69a0b2d68da902a9553743cc77`;
execution used the two source-copy closures above, not that working tree.

Decision: retain the CLI repair with no further behavioral change from this
measurement. The established inert unit fixtures model workflow phases; they do
not execute the actual fixed project/native workflow or introduce a new passing
native adapter. No compiler, native gate/profile, network, installed selector,
certification or held validation operation ran.

The source-bound raw records are the `workflow-differential` subtree of
`alab-validation/8c599ba312f0449b3a18698772d9500931f564cd/feature-bundle-custody-eqrjqn3w`.
That common-Git archive has 340 payloads and 267 source bindings across its
separately scoped packets; these totals are not workflow test counts. Its index
SHA-256 is
`8fc9dbb2daa4dc9f03dd4e721c6da163b82c5fec0bf02c79ede760eb5bbf27a8`.
The differential's original index SHA-256 is
`c2684f140afb195e8b26e0771d03bd19394eff4eedab20e08716f7532173b421`.
Independent read-only review reconciles both actual statuses, all six Git source
bindings, the 33 method identities and all 57 full ordered event objects.
