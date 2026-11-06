<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Turn accounting across account replacement

Task startup normally initializes a new accounting interval. Native replacement
continues an existing turn, so resetting its baseline would omit earlier usage
and reset tool counts and memory-citation state.

The private root snapshot (version 2) now includes the tool count, memory-citation
flag, original token baseline, and accumulated per-model token usage. Freeze
retains the original data after the old task stops; capture validates the
serialized data before suspension and again before native consumption. Installation
still requires original-turn admission. Its one-use startup marker carries the
baseline directly, before normal task startup can overwrite it.

Per-model snapshots contain token totals and the original histogram tags. These
are the tags emitted by the metric API, not the full telemetry object or an auth
grant. Process-local metric clients are never serialized. A replacement emits
through its own client with the original tags, and later usage for an existing
model is added to the same bucket. The ordinary zero-usage fallback is suppressed
when restored buckets supplied the samples.

The focused checks pass: three per-model tests cover repeated replacement,
invalid input, and actual histogram emission; three root-input tests cover
admission, the original baseline, counters, and mailbox phase; the embedded-client
refusal test still passes with the new snapshot. The histogram test uses a fresh
in-memory sink and proves exactly one sample per token category, the original
model label, combined usage, and no extra zero-valued replacement sample.

The full `alab/check.sh` gate passed on source
`200abb4354baccfe058a49de645c9e8b34247029`: the CLI built and ran, all 25
ALab test groups passed (1,542 tests, four ignored), owned lint passed, and
all required hooked upstream library suites passed. These include Core
(2,962 tests, one ignored), exec (70), login (237), and TUI (6,185, five ignored).
The retained log is `exec-turn-accounting-full-gate-retry.log` under the
machine's `.aterm/.ubd` directory.

This change does not restore elapsed-time accounting, the last step's capabilities,
interactive continuations or the rest of the per-child session state.
Unread controller mail is covered separately by
[the controller-mail handoff](EXEC-CONTROLLER-MAIL.md). Those remain required work; automatic ephemeral and review
rotation remain disabled.

The process fixture now has `--token-usage` for the pending-mail JSON scenario.
It supplies nonzero input, cached input, cache writes, output and reasoning
usage on both sides of the switch and checks their sums in the final event.
This probe passes on the earlier `71d149396b` package, establishing that the
external cumulative JSON totals were already retained. It is not evidence for
the new internal baseline or histogram changes; those are checked by the
admission and in-memory histogram tests above.

The private canonical package from
`767a333a1622e30fcf4054e37aeb362d59b1faf1` passed version, layout, signing,
and bundled-tool checks without promotion. Its runtime matches the gated
source; subsequent changes were a test-only lint fix and documentation.
With native host `975a0a0654421f9afb387b1c4019a04155eaa8bc`, real-process
checks passed for nonzero token usage with accepted mail, human output with
lost acknowledgement and changed model settings, an eight-process shared
pool with two account rotations, and a completed child's follow-up on the
selected account. Results are retained under `.aterm/.ubd` as
`codex-accounting-token-usage-767`, `codex-accounting-human-767`,
`codex-accounting-shared-eight-767`, and `codex-accounting-child-followup-767`.

These do not prove continuity for an active child: the maintained
[child continuity probe](EXEC-CHILD-CONTINUITY.md) reproduces an interrupted
child turn even though its parent recovers successfully.
