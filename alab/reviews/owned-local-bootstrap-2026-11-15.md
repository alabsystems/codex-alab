<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Share complete local startup inputs

The original TUI startup assembler returned only an embedded client, so a
retained backend could not reuse its full policy and metadata mapping while
keeping the real runtime owner. Make the constructor result generic and expose
the existing retained owner through the client facade. ALab-owned
PreparedLocalInputs consumes the actual Config, CLI/loader overrides, Arg0,
cloud loader, environment, feedback, databases and network policy into the same
client adapter and retained runtime. It creates no parallel runtime or owner.

The original pure CLI/model/OSS mapping now lives in an ALab-owned helper,
consumed by the existing foreground orchestration and available to backend
preparation. Effective provider choices, explicit models, executable paths,
permissions, cwd, hook trust and writable roots keep their original behavior.
Original target selection and interaction ordering are unchanged.

Two actual runtime controls pass: the factory preserves explicit model/provider
inputs and an ephemeral in-memory thread across physical C1 EOF and C2 attach;
cancelling shutdown observation preserves synchronous input withdrawal and the
same actual completion allocation. Source owner completion does not prove
Native effects settlement. All 45 existing startup tests pass. Production/test-library Tippy has no
owned, spanning, failed-target or missing-script findings. The eight set-aside
diagnostics occur outside the changed code.

Checks use source-selected Trust15698 under the protected explicit unverified
runtime lane on the recorded committed base plus exact seven-file patch. The
subsequent commit verifies the checked code blobs; this is not a clean-commit
application build or certificate.

This is the shared constructor boundary after real preparation. It grants no
Native admission or readiness, serializes no frontend objects into a backend,
and does not activate ordinary retained startup or new-version adoption. The
remaining preparation, exact Native origin/Go, frontend/input handoff and
certified application requirements still gate the complete live-update goal.
