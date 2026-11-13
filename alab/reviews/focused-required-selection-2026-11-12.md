<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Required test selections in focused result checks

A combined libtest command can return success when one filter is misspelled and
another filter runs tests. During upstream intake, `cloud_backend::connection_tests`
matched nothing because the file was loaded as `cloud_backend::tests`; two other
selected cases still passed. The existing count-only focused checker correctly
described those completed suites, and the final feature validator refused the
missing named evidence. Earlier rejection avoids spending the rest of a batch
before discovering the incomplete selection.

Decision: extend the existing focused checker with optional repeatable
`--require-filter` and `--require-test` arguments. Filters must occur in actual
passed names; exact requirements must equal one full emitted name. Guarded runs
also require named counts to match completed suite counts and reject duplicate
identities. Unguarded calls retain their existing count-only contract.

`alab_run_focused` forwards explicit guard options before a `--` command boundary.
Malformed options, empty values and option-looking values are refused before a
log or child command is created. Existing command-only calls remain valid, and
command/pipeline failures retain their actual status. Guard options never reach
the child command.

The existing feature-evidence ANSI normalization and completed-name extraction
now live in `focused_results.py` and are reused by both consumers. Existing
feature-evidence parser exports remain available. Feature evidence retains its
source, hash, selected-name and scenario validation; these guards provide no
source provenance, semantic coverage or authenticated proof.

Validation in the isolated `alab/evidence-guards` worktree, based on
`2575bf884d51bcf0b9e4b977218111f863c03a62`:

- `python3 -I -B -m unittest discover -s alab/tests -p 'test_focused*.py' -v`:
  20 passed. After independent review identified an option-as-value parsing case,
  its rejection was added to the malformed-option matrix and the 20 cases passed
  again in 0.618 seconds.
- `python3 -I -B -m unittest discover -s alab/tests -p test_feature_evidence.py -v`:
  22 passed in 6.061 seconds, including colored output, cursor-terminated TUI
  records, invalid plain-text boundaries and portable log bundles.

The new cases cover zero and partial filter matches, exact-name sibling/variant
refusal, names outside completed suites, duplicate names, count mismatch, ANSI
and multiline frame output, argument forwarding and malformed options. The
capture checks retain command status 7 and capture-pipeline status 9 under shell
nounset. Independent read-only review found no other concrete issue.

Only ALab-owned Python, shell, test and documentation files changed. No Rust
builds/tests, running qualification files, frozen runners, upstream source,
account state or remote refs were changed. Landing is deferred until the active
source qualification finishes; this topic is not itself a qualification run.

## Successful diagnostic output

During subsequent native validation, parallel `--nocapture` output embedded a
real passed completion in another test's debug JSON. The strict named-result
checker refused that incomplete parse. A trial without successful output was
stopped because it would hide self-skip messages from tests that return success.

The documented capture examples now use libtest's `--show-output`: successful
diagnostics remain visible for skip detection while completion records normally
remain separate. The parser is unchanged. The existing skip regression
now also supplies a realistic successful-output section with a fully named
passing test. It accepts benign multiline diagnostics and still rejects every
recognized self-skip message with an exact required test name.

The focused capture/parser suite passed all 20 tests in 0.415 seconds, and the
feature-evidence suite passed all 22 tests in 3.179 seconds on this update. The
parser implementation was unchanged. No native tests, sync-worktree changes,
additional merges or remote updates were performed for this follow-up.
