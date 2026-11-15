<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native fixture publication and child-status evidence — 2026-11-15

Decision: publish the gate fixture's native runner through a temporary file and
atomic replacement, and preserve raw child statuses in structured observations
and failure stderr. These address independently demonstrated publication and
diagnostic gaps. The earlier intermittent receipt failure has no established
cause; neither change is evidence that its cause was found or repaired.

The full Python workflow at `9b3fdf89429e15e767cf060843e484cb9113eafb` ran
1,044 tests in 1,739.474 seconds, with one failure, no errors and ten skip entries:
nine whole tests and one subtest. All three mandatory actual native-context
controls passed. The failed wrong-target subcase stopped earlier than intended:
candidate-only attribution lacked a required CLI receipt, before pristine
metadata admission. Fake Cargo reported only generic nonzero child failure.
No target-admission bypass was observed. All forty indexed files are retained;
record SHA-256:
`2df5bb89698cf9159f801d45405a3ab51c9cc38c94f30b02ee7dfdb098a3fe5b`.

The original method then passed in isolation with both intended metadata refusals.
A private status observer also passed, showing eight expected native child exits.
A bounded repeat ran ten variant checks on one fixture and retained forty exits:
twenty zero and twenty expected 101, with no negative status. These are distinct
executions; cumulative snapshots were not added together. Original, observer and
repeat archives contain 98, 101 and 385 indexed files, respectively, under
`.git/alab-validation/pristine-runner-receipt-next-20261115/`.
The failure remains unreproduced. Additional observation and snapshot I/O do not
prove equivalence to the original failing schedule.

The fixture did overwrite the same runner inode during record and comparison
builds. The source artifact is an ad hoc signed arm64 Mach-O image. Apple documents
that modifying signed code in place can leave the kernel's signature cache
inconsistent and cause hard-to-reproduce crashes, and recommends replacement
through a temporary file. This supports prevention, not an attribution of the
earlier failure. [Apple: Updating Mac Software](https://developer.apple.com/documentation/security/updating-mac-software).

The fixture now closes a temporary file in the destination's parent, copies and
sets executable permissions before os.replace, and removes staging after failure.
It leaves the old image unchanged. A retained hardlink plus a different published
inode tests this even when the bytes are identical. A missing-source control
checks that failure preserves both old names and leaves no staging file.

Every child invocation now records its raw returncode, configured runner argv,
library path, cwd and libtest arguments. Nonzero codes also appear in stderr,
so ordinary fixture teardown cannot erase the only useful diagnostic. Existing
Cargo error text, aggregate 101, selectors, receipt validation and metadata
admission remain unchanged. Debug observations are cleared with the other
per-check observations; they are not qualification evidence.

Committed RED `08e4282b0065aac7496f42bc67e76a4927cdbcda` ran three methods.
The failed-copy control passed; one inode assertion and two raw-status subcases
failed, without framework errors or skips. Its 670-file archive has record SHA-256:
`d4c7f3de8d6376019cb55860845927fed4727626a8159197edb3ae3e5abec042`.
At `49a34d8bc2d0c7ebe9f464de34305add2ecab7c5`, all three methods passed in
6.639 seconds, with zero controller/unit exits and no skips or capture errors.
Both CLI/core observations contain actual 23 or synthetic self-SIGKILL -9,
while aggregation remains 101 and receipt admission fails closed. These shell
controls do not diagnose a real code-signing crash. The 671-file archive has
record SHA-256:
`9bff7e04f1edb852a04eb8dfbc57e8a8defe8614f7297c13ca9a012c3180658f`.

One initial GREEN controller launch used a mistyped path and exited 127 before
tests; its failed-launch receipts remain separate. The corrected launcher reads
the prepared source/archive record.

The fresh full workflow at `9eece152aef9678e5a1dd385f8c08628474631f9` ran
1,047 tests in 1,398.748 seconds: OK, with zero failures or errors and ten skip
entries (nine whole tests and one subtest). All three mandatory actual native
Cargo-context controls passed exactly once. Both controller and workflow exited
zero; the source stayed clean and the support-runner hash stayed unchanged.
The forty-file archive under `.git/alab-validation/9eece152aef9678e5a1dd385f8c08628474631f9/full-python-workflow/`
has record SHA-256:
`35c7f729748b800dfa67bab891afc1a84385c428f8cc80e5680c3681a3937f71`.
Skip entries include a subtest, so no pass count is inferred by subtraction.
Optional native opt-ins, resource admission and 60-second native timeouts were
unchanged. The reused support artifact had identical compiled inputs; this is
Python workflow and native test/identity evidence, not an application certificate.
Raw execution-context receipts remain private. The earlier failed run remains
failed; this later pass does not establish its cause or prove causal repair.

The next hostile review found a narrower diagnostic gap: JSON append happened
before the nonzero child status reached stderr. Decision: print and flush that
known status first. Structured observation failure still aborts fake Cargo; no
receipt or aggregate outcome is invented. Successful JSON logging and ordinary
Cargo failure behavior remain covered by the existing methods.

RED `ac03cc912443bb6b0582641432b23d588a39dae6` added a directory at the JSON
log path and a synthetic shell child exiting 23. Of four methods, only the new
missing-stderr assertion failed; the other three passed, without errors, skips
or capture errors. The 862-file archive has record SHA-256:
`9253d8ca852beceebb169486056776ef7c8934067ddd8413e4f219d50c0c7250`.
GREEN `dce40d71663126a775f661f76faf04cbb2a2eff5` passed all four methods in
10.957 seconds with both exits zero and no errors, skips or capture errors.
The new control retains CLI status 23 and the append exception, while the check
fails closed with no markers or library receipts. It does not require a core
invocation or aggregate 101 after persistence aborts. Its 862-file archive has
record SHA-256:
`4b0036513e95ed5fc848a504db97f081dbc05180ec7c61275785bca43b8ada78`.
These focused results qualify the stderr-ordering follow-up; the earlier full
workflow result keeps its exact source attribution and was not rerun.

All changes are in two ALab test files. Fake Cargo performs no Rust compilation;
its certified-build text is a test fixture, not an application certificate.
Atomic tests copy but do not execute the support image; adversarial shell children
test diagnostics and missing-receipt rejection. No installed application or gate
policy changed. Certified application delivery retains its recorded prerequisite,
and the original backend Bad Request cause remains unknown.
