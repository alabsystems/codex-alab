<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Requested live-update status and bidirectional Git alignment

The human asked whether seamless immediate live updates are complete and requested a
pull plus alignment in both directions. The goal remains active and incomplete.
Complete App/BottomPane state, original editor handoff, Native adoption and the actual
cross-exec update caller still need integration and end-to-end qualification. Component
runtime results and a Git publication do not certify a shipping application.

The candidate merges live-update topic `2743bc6baf110beba66e4cdf397f7853548ad5a6`
with main `73dbb5eae284dd0bfd15f99a9d8038baa12f6a87`, preserving both histories.
It retains all 32 topic-changed paths and all 14 main-changed paths byte-for-byte
against their respective parents. The merge has no conflicts, loses no hooks from
either parent and introduces no new upstream commit. Main includes diagnostics,
review/intake records, the historical-skip correction and narrowly removed unused
helpers; the topic adds native widget transcript and streaming preservation.

Before integration, all 32 unfinished Root worktree paths were copied and hashed
outside Source and retained in Git stash `b800fc847e939c846d00a3e18aff9650728c771b`.
Those incomplete edits are excluded from this candidate and are restored after
publication with their exact bytes and modes checked. They are not qualified by
this alignment. Agent drafts remain outside Source and unmounted.

The broad owner-library execution is explicitly NONPASSING and incomplete:
502 case rows passed and ten handoff case rows failed before it was interrupted
after repeated hang-detector timeouts. Its actual command status is 101. No case
was relabeled and this scope was not used as a passing gate. Read-only diagnosis
identified two production refusals which these package-only positive fixtures
cannot cross: `PreparedExec::native_builds()` requires a numeric installed-package
path for the actual current executable, and `Host::capture` rejects an in-process
FakeAterm host. The test runs from debug/deps. This explains a concrete fixture
blocker, not a claim that every failed case has been individually reproduced or
that the production update mechanism works. No guard was relaxed.

The follow-up scope runs the actually changed `open_work_tests::` module,
retained component controls and focused lint separately. Those results are recorded
below; they do not replace the failed broad execution or certify the application.

Validation on the exact merged source uses source-selected Trust 15698 through the
qualified project controller, with explicitly unverified component execution:

- 993 retained controls pass across twelve libraries; zero failures and three existing ignores.
- All 16 ordinary `open_work_tests::` methods pass, zero failures or ignores.
- All 53 Python methods pass: request timeline 39, verdict protocol five, intake nine.
- Focused Tippy exits zero but has 32 owned diagnostics, zero spanning/failing-target/
  missing-script classifications and nine upstream diagnostics set aside. This is NOT
  lint clean. No application gate, certified package, adoption or end-to-end update is claimed.

Exact input patch SHA-256 is `a65715c0f32761915ad03416ed369e8d50b179ef8218461d9f2d45c27731df17`.
Retained runtime log SHA-256 is `d6e78257bbf823bcea27650dc24aadedc7ff83068e9af94ba643f6e208701123`.
Outside `requested-current-alignment-{focused-tests,python,owner-failures,admission}`
records preserve command statuses, logs, actual counts, parent preservation and hook admission.
Source bytes are checked again after all executions; only this review follows the checks.

Publication uses Agh's atomic multi-ref operation with freshly observed expected
main and topic heads and ancestry checks. There is no force push or history rewrite.
The publication receipt and final local/remote verification live outside Source;
this review cannot claim a successful publication before that operation completes.
