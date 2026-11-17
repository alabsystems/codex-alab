<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Retain qualification cleanup diagnostics at the CLI

`run` retained cleanup failures as exception notes, but `main` printed only the
primary error or incomplete-observation header. An observer stream failure
followed by failed TERM, KILL, bounded reap and stdout closure therefore lost
all four secondary diagnostics at the public CLI boundary.

Decision: print the selected error's existing notes after the existing header
in the ordinary-error and zero/None SystemExit handlers. Reporting is best
effort: formatting or stderr failures must not turn an incomplete observation
into CLI success. Ordinary reporting exceptions and zero/None SystemExit leave
status 1; a reporting KeyboardInterrupt or nonzero SystemExit still propagates.
An unusable diagnostic sink may retain only a prefix or no diagnostic. Normal
returned success, nonzero observer exits, help, resource retirement and record
publication rules retain their existing behavior. The renderer does not close
borrowed streams or claim an unknown child was reaped.

The final 41 maintained cleanup/CLI methods run against coherent source from
exact baseline `794ff49328c17a6f14bae5adca06e051dc47e3e6` and fixed commit
`52968d71cde24bc9db138df212bd20a21d1a5917`, with identical final tests.
Baseline: five assertion failures, 36 passes, zero errors/skips, actual exit 1.
Fixed: 41 passes, zero errors/skips, actual exit 0. The previous 34 methods are
unchanged. New public main-to-real-run controls naturally generate the four
cleanup notes using module-local fake subprocess, pipe and stream objects;
they do not fabricate exception notes or execute a gate. They also cover
header/first-note sink faults, formatting failures, exact interrupt identity,
existing headers and help. No actual signal is sent by these fault controls.

Independent review approved the frozen implementation and controls. The final
combined 95-method run is described in the adjacent
[record-shape decision](qualification-record-shape-2026-11-16.md).

Evidence is retained under common Git
`alab-validation/80adcc3dd308f966ed24889d11ee38732ba27fdf/qualification-diagnostics-3_r8ygiw`.
Its recursive index SHA-256 is
`fd17c25909e4759f6641fc8aab1e1d32f19cd68cefcdb8b136abcfe6ae0a5083`:
1,588 payloads, 22,211,802 bytes, including both coherent counterfactual source
bundles, raw logs, drivers, contexts and final combined input snapshots. All
payload hashes, sizes and exact inventory were checked. Earlier discovery
evidence and the explicit withdrawal of the unproved JSON-depth hypothesis
are retained separately from the final controls. The fixed CLI-only qualifier
hash is `f2947165f6b2cf81ff6ea942ca7ee96fba0e9df3128a1a1531e328d75818af8d`;
the later combined qualifier is a different blob, as the contexts record.
