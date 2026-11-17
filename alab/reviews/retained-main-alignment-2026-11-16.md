<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Align the live-update qualification branch with current main

The person requested a completion check and bidirectional Git alignment.
Decision: fast-forward the clean main checkout through Agh to observed
`2d76c43bb6af215c0a0eea8ab675edb3c7005dab`, then merge that main into the
qualification branch at `2e58b7b27534e61dbfbd895e6eeb2f976b6b524a`.
The merge is conflict-free. Publish the resulting qualification branch without
moving unfinished live-update behavior onto main.

The eleven incoming paths add HTTP body-read progress diagnostics, stricter
feature scenario descriptions and retry-schedule regression coverage. A bounded
independent review found no interaction defect. All nineteen validated retained
selection inputs and both lockfiles remain byte-identical to the topic parent;
no upstream source path or hook changes in this merge.

Source-selected Trust 15698 ran the protected native project's explicit
unverified test lane on the exact merged inputs. The selected outer harnesses
pass 95 API fixture methods, 329 retained SDK methods, 464 owned Core methods,
and six actual TUI startup-account-read methods: 894 distinct passes. Four
existing SDK helper cases remain ignored; 6,472 unrelated TUI cases are filtered
out. The maintained feature-evidence runner separately passes all 79 named
Python methods. These selections do not qualify the full TUI or Autopilot suite.

Scoped Tippy exits zero. The maintained lint classifier reports zero owned,
spanning, failing or script findings and sets aside eight distinct upstream
diagnostics, observed twelve times across compilation targets. Every primary
diagnostic source file is byte-identical to the topic parent. The eleven merged
inputs remain unchanged through all phases. Native build-admission telemetry
could not retain the TUI test build's peak measurement due to an operation
permission refusal; compilation and all six selected runtime methods exit zero.

Exact input patch SHA-256:
`82024c4387d25bfab67fa18865e9035266d542984dc0070c1dee0fef2f882a5a`.
The outside evidence root `~/codex-live-update-evidence` retains
`git-realign-2d76.json`, the matching patch, per-phase logs and controller results,
named Python results, runtime method identities, lint verdict and final
`git-realign-2d76-validation.json`. This decision record is the only addition
after those checks.

The full live-update goal remains incomplete. Ordinary retained TUI entry,
the actual selected and configured UI consumers, pending views and live editor
ownership transfer, and a genuine two-signed-image transition remain unqualified.
The earlier interrupted broad Autopilot run's failures remain separately
recorded in `retained-main-reconciliation-2026-11-16.md`; this focused selection
does not turn that run green. Shipping certification still lacks the required
Trust capability described in `trust27009-adoption-build-script-refusal-2026-11-16.md`.
No application certificate, package, installation or ordinary activation follows
from this merge.
