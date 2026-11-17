<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->

# Unit-result cleanup diagnostics preserve selected errors

Decision: select cleanup exception authority before attempting advisory formatting
or note attachment. Keep the existing policy: an actual cleanup cancellation
supersedes an ordinary primary error; an already pending cancellation remains
primary. Preserve successful note wording, exact selected objects and explicit
cause when a cleanup cancellation is promoted.

The implementation base is `d472bce5ad98ce71bd5f9639df3fa6ba08674bf1`.
The source content is SHA-256
`9d3fa3d28c0b99224b649b151cda42ccd5e107d068adfac82ed2f9722d2c4a6d`.

## Concrete failure and scope

The existing unit-directory and diagnostics-root decisions require pending
cancellation and ordinary primary identity to survive secondary ordinary cleanup
failures. A public `open_directory("/first/second")` audit used the maintained
synthetic descriptor ledger: child open failed, the still-owned parent was
consumed exactly once by close, and its ordinary cleanup error then failed while
being rendered as a diagnostic. An error's `repr` or `add_note` callback replaced
the selected exception with a new plain `SystemExit(0)`.

Eight in-process audit controls contained three healthy controls and five
identity failures. The five included pending KeyboardInterrupt, pending
SystemExit(47), an ordinary primary, and a legitimately selected cleanup
SystemExit(48). All failed ledgers were empty after exactly one synthetic close.
No real descriptor, native runner, application, socket, thread, Git transfer or
signal was used. This is API error masking; it does not establish CLI success,
native admission bypass or qualification. The CLI already treats a plain
success-shaped exit as an incomplete operation.

## Implementation

Only `_cleanup_after_error` changes. Actual exception hierarchies are inspected
with `issubclass(type(value), Exception)`, avoiding foreign `__class__` callbacks.
Authority selection precedes diagnostics. A failing `repr` gets the fixed
`<unprintable exception>` fallback. Formatting, optional note lookup and note
calls are guarded against BaseException. The selected cleanup cancellation is
raised from the exact original primary outside that advisory guard. Other paths
retain their caller's existing bare rethrow.

This generic API preserves its existing promotion policy even for an actual
SystemExit(0). It does not import the guardian's distinct normalization policy or
reject SystemExit subclasses here. Directory ownership, open flags, close counts,
manifest rules, receipt/result schemas and source qualification are unchanged.
There is no new dependency or copied-input edge.

## Memory-only validation while writes are unavailable

The new eight-method test matrix exercises 70 public directory calls through the
existing fixture module alias, without importing TestCase aliases. On unchanged
source it produces 63 expected failed subcontrols across six methods, zero errors
or skips. Healthy note wording and absent optional note transports already pass.
The in-memory candidate passes all eight methods and the existing 36 pure
directory/diagnostic methods: 44 unique methods, zero failures, errors or skips
on managed Python 3.14 with `-I -B`.

The matrix covers diagnostic repr, note-call and note-lookup faults; promoted
cancellation and cause; foreign class lookup; normal notes; and unavailable note
transport. Real open/close and child/thread/socket/signal operations are
independently forbidden and counted zero. Source bytes match before and after.
The candidate full-source SHA-256 is
`e9dfd9774ab55a304d5567ec2d369a6f0e5e465637b4b1493087f12307088c2f`;
the prepared test source is
`37a98ed58ab6aed67c7690ee234d5140a7c454cf9ab6bc29ed2c63225e94c880`.

One ordinary exclusive write probe refused errno 28 before creation. No sparse
worktree, branch, source edit, temporary file or proof deletion was attempted
after that refusal. The candidate and tests remain in conversation memory until
ordinary writes recover. File-backed prepare/collect/status fixtures and the
full native-unit-runner suite have not run for this candidate; no substitute
runner, native build or qualification is claimed.

Root independently matched the exact candidate hash, verified that all other
ASTs are unchanged, passed the existing 36 pure methods, and passed ten additional
public fake controls; six of those ten violate the policy on unchanged source.
The independent hostile review matched the same candidate bytes before executing
24 public fake controls: unchanged source has 22 mismatches and two passing
controls; the candidate passes all 24. Its descriptor ledger is empty after
closing each acquired descriptor once, and foreign metadata callback counts
drop from four to zero. These independent selections overlap the producer's
coverage and are not added as new unique repository test methods.

Both reviews approve this memory-only candidate. Materialized-file verification,
file-backed consumer checks, final test-source review and normal publication
must follow storage recovery before this topic is landed.

## Actual remote materialization and Python checks, 2026-11-17

The preceding memory-only evidence remains historical. This section records
actual file-backed work on the remote Mac. The maintained read command
`python3.14 -I -B alab/github.py --repo ~/codex-alab fetch origin refs/heads/main:refs/remotes/origin/main`
was refused before transfer by native Agh (exit 3, `side_effect=none`):
`background quota admission: Operation not permitted (os error 1)`.
The initial aterm control-socket read also returned sandbox `Operation not
permitted`. Neither refusal was bypassed; no identity, grants, permissions,
compiler/pool state or authority was changed. No fresh authoritative main lease
was obtained. Publication and remote reachability remain unconfirmed.

To complete the independent source verification, one private worktree/branch was
created from retained local `origin/main`/HEAD
`d472bce5ad98ce71bd5f9639df3fa6ba08674bf1`. This is explicitly a retained base,
not a fresh admitted remote observation. Local Git metadata writes needed the
harness's normal per-command filesystem authorization; it did not retry the
native Agh or control-socket refusal. The shared checkout's source was untouched.
The only candidate paths are this note, `alab/unit_results.py`, and
`alab/tests/test_unit_cleanup_diagnostics.py`.

The canonical JSON was decoded once and every stored string verified as ASCII
with final LF, exact byte length and supplied SHA-256 before use. The immutable
retained pre-edit production source has 22,944 bytes and SHA-256
`9d3fa3d28c0b99224b649b151cda42ccd5e107d068adfac82ed2f9722d2c4a6d`.
The materialized candidate has 23,284 bytes and SHA-256
`e9dfd9774ab55a304d5567ec2d369a6f0e5e465637b4b1493087f12307088c2f`.
The new tests have 7,934 bytes and SHA-256
`37a98ed58ab6aed67c7690ee234d5140a7c454cf9ab6bc29ed2c63225e94c880`.
All production bytes outside the specified replacement interval were preserved;
all other production ASTs compare equal. The exact boundary is
`raise cleanup_error from primary\n\ndef _close_directory_after_error`.

Actual checks used managed Python 3.14.8 with `-I -B`. Source hashes and real
worktree HEAD were recorded before and after each run and remained identical.
The immutable baseline replay uses retained original bytes in a separate module,
with in-memory fixture-module substitution; it edits no production source.
It ran eight methods with exactly 63 expected failed subcontrols across six
methods, zero errors and zero skips. The materialized new-test run passed eight
methods with zero failures, errors or skips. Independent fixture observation in
both runs counted 70 public directory calls, 70 acquired synthetic descriptors,
70 closes, every close sequence `[100]`, and zero live ledger entries. The tests
also assert exact selected exception/cause objects and diagnostic callback lists.

The combined prerequisite-independent run passed 89 unique repository methods:
eight new diagnostics, all 36 directory, all 19 preparation, all 16 collector,
and all ten portable CLI/status methods; zero failures, errors or skips. The
repeated eight-method run and overlapping additional controls are not added to
this unique count. Preparation uses its existing inert, never-executed artifact
fixture and synthetic operation ledger. Collector methods use real temporary
status files, a module-local synthetic descriptor ledger and mocked collection
boundaries. These are not real-FD/native-runner tests. Status methods use the
existing Python CLI consumer fixtures; no raw signal command was sent.

All scratch and retained evidence are under the person's home at
`~/codex-alab/personal/unit-cleanup-diagnostics-20261117-remote/`:
`payload.json`, `payload-verified.json`, `materialization.json`, immutable
`baseline-unit_results.py`, `verify.py`, `baseline-replay.{json,log}`,
`candidate-diagnostics.{json,log}`, `candidate-consumers.{json,log}`,
`test-summary.json`, `initial-fetch.{json,stdout,stderr}`, and
`native-readiness.json`. JSON results retain every unique test ID and source
hash/HEAD bookends; scratch is bounded and owned by this assignment.

The full `test_unit_results.py` class requires an actual native
`alab-unit-runner` artifact in `setUpClass`; the default worktree artifact is
absent. It was not executed, no fake runner was substituted, and no Rust build,
native application/security probe, package, adoption, updater or qualification
was attempted. That native readiness limitation is outside this source topic.
The GitHub admission refusal is a separate publication blocker.

## Fresh admitted main and independent materialized review

After the first checks, the m50 AI supervisor conditionally clarified use of the
approved full-access tool route for Agh, subject to policy permitting it and
honoring any refusal, with the same identity and native checks. No person
approved that tool-route use. The maintained fetch then succeeded (exit 0) and
freshly admitted
`origin/main` at `f6b68ad38359feeebeb3c9971983a28c1218232b`.
`approved-fetch.json` and its stdout/stderr retain that result. This is a later
successful observation, not a relabeling of the earlier sandbox refusal. No
identity/profile, fleet grant, installed permission state or native check was
changed. The aterm control-socket refusal was not retried.

All nine compared inputs (production source, four existing test modules,
`cli_status.py`, both GitHub helpers and AGENTS.md) are byte-identical between
the retained base and this incoming main. `incoming-comparison.json` retains
their hashes. The single private worktree was fast-forwarded to that freshly
admitted main, preserving every incoming commit and all three candidate files;
`incoming-integration.json` records the exact before/after OIDs. No handler
overlap needed resolution and the approved production/test hashes still match.

Two native agents independently reviewed the exact materialized production,
tests and initial remote note (note SHA-256
`5470ce2485478b4f0eee44b2614cbcb5dde9dc03d8597a662a2f27e45d6b5e7d`),
with no findings. Both verified the canonical bytes and historical review
prefix. The production reviewer verified the exact replacement boundary and
unchanged other production bytes/ASTs, then passed 612 public directory controls
and eight direct hostile-phase controls using Python 3.14 `-I -B`. Those cover
actual SystemExit(0), SystemExit subclasses and BaseException families, hostile
repr/note/class metadata and phase formatting, exact selected objects/causes and
callback lists. All 612 acquired synthetic descriptors closed exactly once,
with zero live entries. Its selected 44 repository methods passed with zero
failures/errors/skips. Evidence: `production-review.py`, `.json`, `.log`.

The independent test/note reviewer passed the eight diagnostics methods and all
16 collector methods, reproduced the baseline's 63 expected failed subcontrols
across six methods with zero errors/skips, and independently checked both
70-call ledgers (70 acquisitions/closes, each `[100]`, zero live). It verified the
producer's full 89 unique method IDs and counts. Evidence:
`tests-note-review-check.py`, `tests-note-review-report.json`, and its baseline,
candidate and collector logs. All these independent selections overlap the
producer's coverage; the repository unique-method total remains 89.

Publication will use the maintained `alab/github_push.py` with a fresh complete
main OID as expected lease and the existing full-history/canonical guards.
Actual publication receipts and remote reachability must be observed before
retiring the worktree/branch; those later facts will be appended separately.
The source-only native-artifact limitation above remains unchanged.

## Actual source publication and remote confirmation

The exact three-path source candidate was committed as
`112411899ed18536a7ca9e11b1cc2030e1ad5e5c`, with parent/fresh admitted base
`f6b68ad38359feeebeb3c9971983a28c1218232b`, subject
`alab(unit-results): preserve cleanup authority through diagnostics`.
Before committing, both native reviewers approved the prepublication three-file
identity including note SHA-256
`23e95c8d94b9a008b188f79a23473147988bad16da40f739ad6f1a60e11abfb9`;
`production-review-prepublication.json` and
`tests-note-review-prepublication.json` retain those approvals. The final clean,
committed check passed the same 89 unique methods with zero failures, errors or
skips at that exact candidate HEAD. `final-committed-check.{json,log}` retains
the unique IDs and identical committed-byte/hash/HEAD bookends. These repeat
checks do not increase the unique-method count.

A new maintained fetch immediately before publication confirmed authoritative
main still at the full expected lease
`f6b68ad38359feeebeb3c9971983a28c1218232b`.
The approved same-identity full-access call to maintained
`alab/github_push.py` succeeded, exit 0, with full-history and canonical guards
intact. Its native receipt identifies `op=ref.update-atomic`, `outcome=created`,
`dry_run=false`, repository `alabsystems/codex-alab`, and intent
`3-fa1e86a51a45`. Reading that native intent confirmed routed identity `dev`,
`open=false`, `settled=created`; no identity or routing was substituted.
No force push, rebase, upstream push or duplicate write was performed.

A subsequent maintained fresh fetch independently confirmed actual remote main
at `112411899ed18536a7ca9e11b1cc2030e1ad5e5c`. Local ancestry and exact
remote-main production/test bytes were then checked against the approved hashes
above; both match, and every incoming main commit remains an ancestor.
Retained evidence: `source-commit.json`,
`prepublication-fetch.{json,stdout,stderr}`, `source-publication-request.json`,
`source-publication.{json,stdout,stderr}`, `source-native-intent.{json,stdout,stderr}`,
`source-remote-confirmation.{json,stdout,stderr}`, and
`source-remote-identities.json` under the home evidence directory.

This publication-fact appendix is a documentation-only follow-up within the
same three-path topic. Its own final publication receipt, reachability check and
ordinary private worktree/branch retirement will be retained in that evidence
directory after this note is independently reviewed. Source correction remains
a Python source result, with no native binary/package/qualification claim.
