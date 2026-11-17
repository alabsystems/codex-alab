<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Requested live-update status and bidirectional source alignment

The human asked whether seamless immediate live updates are complete and requested
another pull and alignment in both directions. The goal remains active and
incomplete. Actual App/outer-loop integration, the retained C0 backend/read host,
physical IO retirement, genuine Native editor/C2 resource issuers, complete
App/BottomPane/frame state and certified package/activation still require work.
Component tests and Git publication do not establish completion.

Fresh Agh reads found main at `e85ee16eec9f07b17b431c6514fca017aa06d72a`
and the live-update topic at `e0a6b7a284601487ae5208a5d4b63ea54f206e14`.
Their common base is `f6b68ad38359feeebeb3c9971983a28c1218232b`;
main has eleven commits absent from the topic, and the topic has one absent from
main. The merge preserves both histories without rebasing or force pushing.

The topic changes 24 paths; main changes eight. There is no path overlap or
conflict. Independent native-subagent review and Root's separate full index
comparison verify exact parent blobs and modes across all 12,325 tracked entries.
The entire Rust tree remains identical to the tested editor-resource component.
Incoming production changes harden completion-receipt cleanup and unit-result
diagnostic authority; the other incoming paths are tests, reviews and intake
metadata. A failed diagnostic callback cannot replace selected failure authority
or prevent subsequent owned cleanup. No new upstream implementation is integrated.

Seven unfinished local C0 read/stdio files are excluded from this alignment.
Their exact patch, file hashes and original base were saved outside Source before
an explicitly named stash. They will be restored and compared byte-for-byte after
publication. Native work and outside App/Pane drafts remain separate.

Source-selected Trust 15698, through the qualified protected project controller,
ran the maintained thirteen-library retained-state scope on the exact merged
input. All 1,123 methods pass, with zero failures and three existing ignores.
Their passing identities match the preceding editor-resource component exactly.
All 244 selected Python methods also pass across ten invocations, covering
command/receipt cleanup, unit-result diagnostics and actual native-runner use,
directory ownership, intake admission and feature ownership.

Merged input patch SHA-256:
`82057515d225f8d38d703d3e48c666bf4011ee07e9680aae805815bc3f35b54e`.
Retained runtime log SHA-256:
`e611f1e460e8a09852031640f406411f66c64a56fe817179bf16511be261c3ac`.
Outside `requested-alignment-e0a6` records retain exact inputs, runner/controller
identities, logs, statuses, the full parent preservation check and method counts.
Only this review follows those checks; executable/configuration bytes are unchanged.

The first Python invocation of test_unit_results stops in setUpClass with zero
methods because the default target/debug runner path is absent. That invocation
is explicitly nonpassing. The actual native runner exists under the explicit
project target; its identity is recorded separately before the corrected run.
No missing or failed result is relabeled as a pass.

The preceding component has 1,004 owned Tippy diagnostics. This alignment is
not lint clean and makes no shipping-gate, certification, package or installed
adoption claim. Atomic two-ref publication requires fresh exact expected heads,
fast-forward ancestry and a clean committed candidate. A subsequent fresh fetch
establishes remote alignment; this review does not assume publication succeeds.
