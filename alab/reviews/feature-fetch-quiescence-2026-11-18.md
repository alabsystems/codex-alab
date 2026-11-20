<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Retain the original experimental-feature popup fetch

A popup feature fetch waits behind an accepted feature save before starting
discovery. Its wrapper was detached with `tokio::spawn`. Discovery already uses
the Session's descendant-aware helper, but outside a tracked parent that helper
falls back to ordinary scheduling. The waiting wrapper and its later child
could therefore remain outside the owning Session's work Book.

Commit `0725affbb788571ddbafcf4346f4c8ec085e62ee` replaces exactly that one
original wrapper spawn with `app_server.alab_spawn_original_work`. The SAME
future, actual feature lock, request handle, thread and popup result sender are
retained. Reversing this one marked call restores every upstream-file byte from
parent `55f75cf85c6109bcee0ed1192248375d64239487`; tests and their mount live
in ALab-owned files. No upstream body was reformatted, and no helper, accepted
save hook, retry, duplicate discovery or new resource owner was added.

The original `select!` still chooses actual lock acquisition or popup closure.
Closing the popup while the lock is held ends its wrapper without an RPC.
After acquisition, the original helper keeps its five-second timeout, request
ID, pagination bounds and cancellation policy; the timeout is not moved ahead
of the lock wait. Normal shutdown and capture now retain this same root and
its original descendant.

The maintained native unit invocation passed all three selected TUI methods,
with no failures or ignored methods and two complete zero-match siblings.
Actual process, typed guardian, test, collector, coverage and source statuses
were all 0: original `48947`, start `234d5c`, completion `d85ad6`. The new
actual-App-entry method completed three scenarios using real owned
`CloudClient::channel` transport and the same feature lock:

- Open popup success and RPC error: lock-wait capture remains Pending; cancel only its observer, release the original guard once, and hold the actual discovery RPC. Real child admission must invalidate the old cut with WouldBlock. A fresh cut of the SAME Book stays Pending until one response, then produces Ready/current and the original popup result or error without another fetch.
- Popup closes while the lock remains held: the same accepted root settles with a stable cut and zero discovery RPCs.

The two existing companions cover selected server profile/current task settings
and pagination/bad-server bounds; the lower feature-WRITE method was omitted.
Scenario cells are not extra methods. Explicit test failures use ordinary
Result cleanup of the same transport/Session; unexpected panic or abort cleanup
is not claimed. Identical before/after books bind 12,529 source inputs and
11 tool identities. The unit result is separate from the full gate.

The canonical gate on the same `0725` source failed at full Tippy: actual,
guardian and qualification statuses were all 1, with `qualified_source: null`.
The complete classifier found 954 source warnings (951 dead code, 3 unused
imports), 0 source errors, and 2 separate locationless notes. The normalized
multiset exactly matches `3c6`: 0 diagnostic identities removed and 0 added.
Certification, application runtime and the later full test phases were not
reached; the focused unit pass does not establish those outcomes.

The maintained guardian -> qualification -> executable no-argument `check.sh`
route used source-selected Trust15698, native target admission and protected
profile. Supported `ALAB_CHECK_BASE=55f75cf8...` stabilized hook comparison only;
intake and qualification still observed actual `origin/main`, with no hook-loss,
proof, compiler or profile exception. Source/tree stayed fixed and clean, but
qualification recorded its origin base advancing `f722cb3b` -> `bd14be4e`, so
the whole before/after context is not described as unchanged.

This finite boundary does not migrate a Rust future into C2, grant Native/Source
authority, assemble a whole App scene or certify an application. The local
transport fixture supplies no fake SDK owner. Whole-App consumers and separate
App-only timer/listener contracts remain unfinished work.

Evidence:

- Unit summary: `.ubd/codex-feature-fetch-summary-6mtg1m0n/closed-summary.json`, SHA256 `16b2fb81ebe890c8c1ee0c0f519d19067f89ecf768a37cc7a274ad9f5c1ccf6c`.
- Unit seal: `.ubd/codex-feature-fetch-evidence-b7i3e21r.noindex`, index SHA256 `d041d0092d919fd4ad0ed8c0ad3e64dc7e4dcad1f62e2087f1aaffb5b751e038`; Native/guardian contexts retained opaquely, no archive execution or binary/cache/full-source copy.
- Gate original `85121`, start `dd49ce`, completion `e1e88b`, actual status 1; `2026-11-18 11:01:03.156375` to `11:04:18.159369 UTC`.
- Gate qualification/log under `.ubd/codex-feature-fetch-widest-output-3hf_msea`; log SHA256 `e6e45eb85435b6081ecd13981950a985ebbcb6b08cad099d7de53da965b54de8`.
- Same root `lint.audit.json` SHA256 `86a4e6f0949d2fefcdc777b75822e89396dbb611a42f333264c268f077f0c96b`; `delta-from-3c6.json` SHA256 `5000adc597ab665d23edd4772dac420d5de2c0854520c800daaa7a6b4a1293ee`.
