<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Review the next forty upstream commits

The reviewed range starts after integrated upstream
`c1934fc99e6b40ddff4261c910ce7733e3abea26` and ends at
`ce3031018ef130e653ed4d9c5bb971652de97161`. Its ALab base is
`7c144752ea20afde9ded3cf97f6d800c89aa95d8`. The adjacent intake ledger
records each commit's actual changed paths, behavioral decision, affected
contracts and source evidence. Review decisions do not qualify execution or
authorize package adoption.

Three independent source reviews cover consecutive parts of the range. The
integration must preserve the existing account, handoff and composer contracts,
including interactions that merge without a textual conflict. In particular:

- Saved assistant-message delivery records must retain upstream's new source
  metadata through the private handoff representation.
- HTTP fallback must retain the existing wake and retry-status hooks.
- Cloud stream reconnection must not invent a seen turn or active goal. An idle
  reconnect owner must not retain counted account activity; each attempt needs
  the original source binding and fresh activity admission.
- The native footer layout must still budget the ALab goal and account rows,
  draft cursor and account click target under constrained height and focus
  changes. Removed upstream layout state must not survive as a parallel owner.
- Upstream's generated protocol archives need regeneration from the composed
  Rust definitions. Removed CI and Bazel paths remain removed under the existing
  path policy.

GCP incremental tools are adopted as a recorded provider-specific default:
they affect requests to an already selected GCP backend and do not introduce a
background request. Experimental multi-environment MCP remains opt-in. The
range grants no Source V1 activation and does not supersede that existing hold.
New telemetry helpers must preserve explicit configuration and ALab's disabled
telemetry defaults. New advisory suppressions are unresolved upstream dependency
debt, not evidence that the vulnerabilities were repaired.

Before integration, a native selector preflight successfully authenticated the
committed Trust 15698 payload, tree
`9e3ecca1051a2231f7a9ed4fce5f5904f77e5f43b0f9dd18da59204efe3040bb`.
The frozen `atpkg-publish` executable has SHA-256
`9c47dbaabf60aa9380eca1a0f84656d6dabfb9bcd049dea3c17e8f4427c84545`.
The command ran `/usr/bin/true`; it is a compiler-selection observation, not a
build or test result. The integration still requires exact-source scenario
evidence followed by the complete native gate before it can land.
