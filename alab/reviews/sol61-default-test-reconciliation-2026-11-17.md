<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep existing profile tests consistent with Sol 6.1 Ultra

Commit `36b331355cc5cc9c133968249bd77c039608d1b5` restored the human-requested
packaged `gpt-6.1-sol` and retained Ultra reasoning. A read-only audit at
`53ea901e3889cbc17053f6eb775e3bc57c2e85c8` found two stale assertions in
`alab-core/src/kimi_tests.rs` that still expected no base model. They came from
the earlier unpinned-family default and contradict the restored packaged value.
Decision: change only those assertions to require `gpt-6.1-sol`. Keep all existing
checks for Ultra reasoning, opt-in Kimi providers, explicit K3/High profile
overrides, merged profile behavior and unchanged base defaults. No test is added
and no production behavior changes.

The same source audit found no demonstrated execution override of an explicit
model by the family resolver. Catalog resolution requires `Config.model` to be
`None`; the manager's explicit-model path, TUI startup and service-tier selection
prefer the configured model. The serving catalog can advertise its family
default independently of a configured pin; that observation is not proof that a
pin was overwritten. Existing family and remote startup tests deliberately clear
the model for unpinned scenarios, preserve explicit Sol and off-family model
checks, and isolate process-global activation through the exact-test child
harness. This is a bounded source review, not a runtime guarantee.

Static verification used managed Python 3.14 to parse the actual packaged TOML:
the base is `gpt-6.1-sol`/`ultra` and does not select a Kimi provider; both
Kimi profiles retain `k3-256k`/`high`. An exact whole-file comparison confirms
that only the two stale assertions changed; scoped `git diff --check` passes.
The source HEAD was unchanged across that check. Retained evidence is
`~/.codex/.ubd/sol61-default-test-audit-20261117.json`. Neither Rust test was run.
No Rust compilation, formatting, tests, tippy, certified gate or package build ran
for this change: the separate native admission probe refused host resource custody with
status 75 and qualified publisher lookup remains unavailable. No substitute
runner, compiler override or permission bypass was used.

Root independently repeated the exact two-assertion source comparison and
parsed the actual packaged TOML, confirming Sol 6.1 Ultra plus both Kimi K3/High
profiles. That static check is retained in
`~/.codex/.ubd/sol61-default-test-root-review-20261117.json`; it is not Rust
execution or release qualification.
