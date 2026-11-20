<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Cover popup-fetch retention during actual Session shutdown

The new shutdown control and existing fetch control passed on exact source
`474a23c069ab9ad85f5159b00f91aab5d401a604`. The final full gate on the
one-line test-style follow-up `c43c6a25e180fd637c8c8173b691d0f566135c2c`
still failed at Tippy with 952 source warnings and 0 source errors. No
application certification, runtime or later full-test phase was reached.

This topic adds one method to the existing ALab-owned fetch test file; it makes
no production, upstream hook, mount, API, timeout or policy change. The control
holds the actual App feature lock and invokes its real popup fetch once on an
owned Cloud typed transport and physical Session. One pinned consuming shutdown
future is driven throughout. A request-only view probes the SAME work Book;
it never substitutes for physical ownership or shuts down another connection.

An unanswered Cloud Shutdown command would be a false retention oracle. The
test therefore consumes and marks the first work-close notification before
answering that original physical command. Only then does it require a NEW
notification from work.drain()->close(), after physical close and the fresh
model Book drain. The independently retained worker progresses while its
observer is temporarily unpolled. With the actual lock held and popup open,
that SAME shutdown must still be Pending. Closing only the popup must then
settle it while the lock remains held, without discovery or another shutdown.
The current_thread ordering prevents a delayed empty drain from impersonating
accepted-root retention.

Explicit Result paths close the popup before cleanup. The actual lock-Arc
baseline is used only to retain an old untracked wrapper on a failing control
path; it is not a readiness test. Completed futures are tracked and never
repolled, and cleanup services the same original transport/shutdown. No panic
or process-abort cleanup guarantee is made. The old untracked negative control
was NOT executed; its described sensitivity is source analysis, not a measured
legacy failure or evidence of a production defect.

| Measurement | Actual result |
|---|---|
| Native2 on `474a` | Original `94856` / `166e04` -> `091d35`; process, guardian, test, collector, coverage and source statuses all 0. Exactly 2 TUI methods passed, none failed or ignored, plus 2 complete zero-match siblings. The new shutdown control and existing fetch method's 3 scenarios completed. |
| Initial full gate on `474a` | Original `48104` / `e379db` -> `8fd80a`; process/guardian/qualification 1 at Tippy: 953 warnings = 949 dead code + 3 unused imports + 1 needless_return, 0 errors, 2 separate locationless notes. Source/tree stayed clean and fixed; qualification recorded origin advancing `d1c17d36` -> `8627703a`. |
| Post-fix full gate on `c43` | Original `47070` / `eaf172` -> `6c0dc5`; process/guardian/qualification 1 at Tippy: 952 warnings = 949 dead code + 3 unused imports, 0 errors, 2 separate notes. Before/after source/tree/base/clean records matched; qualified_source remained null because status was 1. |

The initial gate found only one new style issue in this control. Follow-up
`c43` replaces a redundant return with the same tail Err expression inside the
inner async/select branch, retaining the same shutdown_finished assignment and
Result cleanup. No redundant unit run was added; the Native2 pass remains tied
to exact `474a`. The measured post-gate delta removes only needless_return and
adds nothing. Relative to `0725`, the two formerly-unused changes methods are
now exercised by this real test and their warnings disappear; this is not a
whole-scene production-integration claim.

Both gates used the maintained guardian -> qualification -> executable
no-argument check.sh route, source-selected Trust15698, protected profile and
native target admission. Supported literal
`ALAB_CHECK_BASE=516d16d00563b721da4f63fc09864e581034cd52` controls hook
comparison only; intake and qualification still observe
actual origin. No hook-loss, proof, compiler or profile waiver was introduced.
The final record binds source `c43`, tree
`d390cc2b3bf4fe5c70ef0b0ab1b200fdaf659461`, and origin base
`8627703a6c3622be53e9f7b8e260551f45629a4e`, all clean and unchanged.

This is bounded test coverage using existing ownership. It supplies no fake
SDK owner, Native/Source grant, generic future migration, whole-App scene
assembly or application certificate.

Evidence:

- Native2 summary `.ubd/codex-fetch-shutdown-summary-imbstc11/closed-summary.json`, SHA256 `f2fc7a6f4437f3065c24220c1ddab154990a1241da0a368dcaf7ea834b8325d7`; identical books SHA256 `4358653da5930517d47f8fb951ba919d1f5f379984a08c9563ede3326b95d166` bind 12,532 inputs/11 tools.
- Finite unit seal `.ubd/codex-fetch-shutdown-evidence-u8ska4e4.noindex`, index SHA256 `4e3a9f1dbdabe2cc1f2f54b083b83f1a52f9edf52bde2ee6a00d9052239a2cf0`; Native/guardian metadata opaque, no archive execution or binary/cache/full-source copy.
- Post-gate records `.ubd/codex-fetch-shutdown-post-c43c6a25-9i43jv8l`; `11:40:22.062593` -> `11:43:02.633862 UTC`. Gate log SHA256 `5fded73bc3880bcc29b3a6dab1c5a6c09d703f4aa2a1a42e0479b6beefc58237`.
- Same root audit SHA256 `78738c1ee36b73d083b840204547d9492766950454433399d3711d76413522b2`; delta-from-474a.json SHA256 `4abe3a5750dbf9ceeedd5dede03d1d9ece32418ef06a8acb122d34d87963ecd2`. Earlier failed gate remains separately preserved under `.ubd/codex-fetch-shutdown-widest-output-y813hlbx`.
