<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native prerequisites precede the complete Python workflow — 2026-11-15

Decision: discover and preserve the full test set, verify its mandatory native
controls first, then run every remaining case. The workflow preserves the
current fixture's command, admission, environment and 60-second deadline.

The earlier full workflow at `214a6704c13a83e0a6d3b1fb49b23b3296dacf9a` failed
after losing its timeout output; later focused native controls passed at
`779eb1cd325b3ab00cf8448598683bfb4e84bcea`. A new full default-order run at
`702431f83ab2b1eb1b17f27bf9cb233f56d674a9` counted 1,049 tests in
1,840.100 seconds: zero test failures, one fixture setup error and ten skip
entries (nine whole tests and one subtest). Both exits were one. Its three
mandatory NativeRunnerTests methods did not execute. The source stayed clean.
Its sixteen-file record hash is
`b345808bd9fff1212889d64948a1c407c9db5202daac63fefa1fffa4338497cf`.

This time the retained cargo02 JSON records timeout, null status and 60 seconds;
partial native stderr shows the no-run producer waiting for admission behind
another project's build. That explains this attempt's setup wait, not the
earlier source214 failure whose partial output was lost. The canonical at702
attempt remains failed. It exposed a workflow gap: an unavailable prerequisite
was encountered only after expensive independent test bodies had run.

`alab/python_tests.py` fixes the orchestration rather than relaxing that bound.
It discovers the fixed tree once, rejects discovery errors, duplicate IDs or
missing mandatory controls, and partitions the entire registered native class
before all other cases. A different class spoofing its names cannot satisfy
the gate. There is no filter or CLI option to weaken it. Discovery still imports
test modules before execution; it promises no absence of import side effects.

Two ordinary TextTestRunner lifecycles finish native class/module cleanup before
deciding whether remaining cases may run. Native methods must all succeed,
without skips, expected failures or cleanup errors. A failed native prerequisite
returns failure with the remaining case IDs unrun. Every remaining case uses
normal unittest outcomes and no failfast. Exact started/finished IDs and normal
runner completion govern full coverage. A started last case interrupted during
cleanup cannot manufacture completion. Summary output failure preserves an
already pending exception; a fixture's zero SystemExit becomes CLI failure
because the workflow is incomplete.

The stdout manifest and final summary bind all discovered IDs and the native
subset with a digest. Counts come from actual per-phase outcomes; the final
unittest footer belongs to the remaining phase, not the full workflow. A full
success requires both phases and complete case coverage. Declared non-native
skips remain explicit and successful method callbacks are not inferred by
subtracting skip entries.

At `2f70f3dadc10711d2e65b2f6ae831e8cf4a302cf`, all nineteen inert orchestration
controls pass in 0.004 seconds without failures, errors or skips. They exercise
cleanup, stable order, missing/duplicate/spoofed cases, fixture failures, skip and
expected-failure refusal, pure-case continuation, interrupts and output failure.
The two-file record hash is
`741848c54bb65336cd664a7637fa0613b91a156c77d1adab302a6974c10a8fbb`.
These controls alone supply no native or full-workflow pass.

The complete configured workflow at source `2f70f3dadc` passes. Its manifest has
1,071 unique discovered cases: three real native methods passed first, then all
1,068 remaining cases ran. There are zero failures/errors, ten explicit skip
entries (nine whole tests and one subtest), and no missing or unfinished IDs.
Both phases completed ordinary cleanup, full_scope and success are true, and
both exits are zero. Independent review verified all 41 retained regular files,
recomputed the manifest digest and exact native-first order, and checked the
three native outcomes in the actual log. Record hash:
`b9b40bd39f1292ec73ce38dadf909d190c5cd9af1acd67b8ab541cb71e13d499`.
This does not relabel the failed default-order source702 attempt.
Main subsequently advanced to `c769ed91b987f6769d50dbe767b6a031272a5f5a` with
native diagnostic export, producer and context-output-layout changes. Those
interfaces are used by this workflow. Composition
`c5322022b41660536c3dbe818383bfb5014a7e15` therefore receives a real integration
replay rather than relying on the earlier result. The support producer builds
from this current locked source; the prior0268 image is not reused across its
changed crate inputs. Its selected support image has SHA-256
`95e17f4d984d2102ee0fe6483b27c7682416df5ddeebc6fdc2c771efa4b01fdf`.
The integrated fixture explicitly owns its build output and tests the adapter's
intentional replacement of its qualified diagnostic-directory environment
variable. The workflow inherits that current fixture contract; it introduces
no environmental relaxation.
The actual integrated workflow completes with 1,079 unique discovered cases,
all started and finished exactly once: three native controls passed first, then
all 1,076 remaining cases ran. There are zero failures/errors and eleven explicit
skip entries (ten whole methods and one subtest). The native phase has no skips;
both phases completed ordinary cleanup, full_scope and success are true, and
both exits are zero. Successful-method callbacks are 3 and 1,065 respectively;
the skipped subtest is not inferred to be a skipped whole method. Optional native
probes, the non-UTF8 filename subtest and certification-dependent controls retain
their declared skips. None of those skips qualifies its excluded behavior.
The forty-four-file record hash is
`94b70466ea07ed02232a6b949dc7d28c3905cb42918cfdbbcaca6dfdf772c397`.
Independent review verified all 44 files, the actual fresh producer artifact,
manifest digest, exact native-first case coverage and context/diagnostic receipts;
raw native receipts remain private. This current-source result completes the
previous full-workflow follow-up without rewriting any earlier failed result.

The source2f70 run reuses the admitted source702 support image with SHA-256
`0268f1f6ceccc137b984e6886fbef3c423515327fadd1f2b8c6abf53019d2694`.
The complete previously declared support crate/workspace lock/manifests/compiler
inputs are byte-identical through source2f70, and the actual image hash is
checked. This is support-artifact reuse, not a new producer or proof claim.
The native fixture's actual Cargo commands still use the original qualified
source-selected Trust 15698 prefix/profile, with optional native opt-ins absent
and timeout60 unchanged. Raw context receipts remain private. No application
build, certificate, installation, global clock change or original HTTP400 cause
is established. The unchanged certification prerequisite stays deferred.

Final composition and review retain the exact-source boundaries above. Incoming
main `7948fa2eae59cf771c3504dbf5241051a44aa5f5` adds optional normal guardian
completion receipts and their tests; it leaves the native producer, recovery
fixture and orchestration sources unchanged. Composition
`8dce0ebde6e064c703374b42fcb3185080dbaef9` passes all 64 selected orchestration,
receipt and existing guardian tests, without failures, errors or skips. Its
record hash is
`bae7bbb996ad7804daa4258196db2876209b570ab240158e37dc9ffbb99778b5`.
No second full Python or Rust sweep is inferred from that focused check.

Hostile review then found a separate successful-exit path during discovery:
a real module's load_tests callback can raise SystemExit(None/0), escaping
unittest's Exception-only callback handler before our execution-phase exit
normalizer. At source8dce both forms escaped as success-valued SystemExit and
emitted no manifest or summary; an uncaught None/zero value would exit the CLI zero.
Those probes observed exceptions, not a separate child process status. This
cannot relabel the c532 success: that observation retains both required records
and every discovered case outcome.

Repair `4d4400424e618858d5b76ae1e802478a56584407` catches only successful
SystemExit from discovery and emits the ordinary failed discovery/preflight
records with no cases executed. Nonzero SystemExit and KeyboardInterrupt retain
their original exception identities. Two new controls create actual temporary
load_tests modules through the real discovery loader and restore module/path
state. The first counterfactual run had a test-owned follow-on error and did not
reach both successful-exit variants; its original failed log is retained without
relabeling it. Test repair `6a5fcda621d737765ca1451732cbb61ded590a6c` keeps
all assertions inside each independent subtest and captures an escaped exception
before inspecting the required records. Against source8dce the corrected controls
run two methods with two failed subtests, separately None and zero, and no
framework errors; against the repair both methods pass. The nonzero/interrupt
identity control passes in both observations. The three-file regression record
hash is `5ba19823d547aae094655dff7dfa008336367fa1a48b732830912974126f2ac2`.

On clean source `6a5fcda621d737765ca1451732cbb61ded590a6c`, all 66
selected controls pass: 21 orchestration tests, 14 receipt tests and 31 existing
guardian tests, with zero
failures/errors/skips and actual exit zero. Its two-file record hash is
`7f787bed93ffb6276493b90f6cd8b08dae149379cb9b84e0c82ec450cdab7f29`.
The repair touches no successful-discovery/native execution path; these focused
controls cover its demonstrated failure path without claiming a new complete
workflow, application certificate or installation on this later source.
