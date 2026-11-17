<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Original editor resources and terminal handback

The retained editor path needs the real original command, document, child and
input/mode owners across observer cancellation. A serialized PID, pathname or
finished flag cannot supply those resources. This component retains them locally;
it does not activate or qualify a complete cross-exec update.

Original editor preparation now has one shared factory. Ordinary run_editor keeps
the same directory/sandbox/symlink checks, candidate order, parsed arguments,
inherited cwd/environment/stdio, status handling, read policy and TempPath cleanup.
The retained producer can move that actual Command and TempPath once. Its bounded
registry owns accepted spawn/read workers, the successful Child and cached native
outcome independently of a canceled observer. Canceled joins return their exact
handles; callback application and retirement remain once-only. Whole Drop guards
move into the actual async and blocking workers before polling.

A synchronous BeforeStart holds the original lifecycle mutex through parser
stop/join, mode-owner installation and publication of the complete or partial
resource bundle. Accepted Launch can schedule the same worker before returning
to an update selector; repeated observers reuse it. Definite cancellation issues
an operation-bound receipt only after exact registry removal under that gate.
It cannot fabricate Finished or authorize Native adoption.

The broker retains queued and partial parser bytes, including surviving query
subscriptions. Restoration refuses late or invalid state while returning the
same supplied File and native error. The original pre-child mode preparation is
checked under the mode lock against actual running modes; later editor-changed
modes remain valid under the same operation/fence. Generic mode and panic cleanup
cannot release an active editor owner. Refused partial preparation retains its
actual parser; a genuine no-child receipt can restore saved running modes before
the original Tui post-policy and parser resume.

The original Tui pre/post handoff is shared by ordinary awaited programs and the
owned job. Its actual event source, image cleanup, alternate-screen policy,
stderr policy, cursor invalidation and kernel typeahead flush keep their original
order. Failed/interrupted handback retains its restriction. Borrowed terminal
startup refresh updates native presentation data while retaining the same input
and mode fence. The new retained-editor hook topic belongs to the existing
exec-handoff feature, with no complete-frame or Native capability advertisement.

Source-selected Trust 15698 ran the explicitly unverified scope through the
protected project controller on base
`f6b68ad38359feeebeb3c9971983a28c1218232b` plus the 23-file patch
`b069a31a14996a5141d1b2baf3c47d67ea0b759b9f433a997855b35af6832e11`.
All 1,123 selected methods across thirteen libraries pass: zero failures and three
existing ignores. The preceding 1,088 passing identities are preserved, with
35 additional executed identities, including original editor regressions and
real Child/PTY/parser/descriptor/error controls. These are selected component
methods, not all upstream tests or end-to-end application qualification.

Runtime log SHA-256:
`ac6c8977c3b75135b5610d37f5ff3a9fea6eee0539f7767199f9cbe7cfc06884`.
The exact inputs, runner/controller identities, logs, statuses and method audit
are in outside `original-editor-native-scene8` records. The development controller
was rebuilt from clean native source with its own pinned Trust 27009 after its
prior executable disappeared. Actual application selection remained Trust 15698;
no source pin, compiler override or admission boundary was bypassed.

Focused Tippy exits zero but reports 1,004 owned diagnostics, zero spanning,
failing-target or missing-script classifications and twenty upstream diagnostics
set aside. This is NOT lint clean and does not pass the shipping gate. Two new
style findings were corrected and the complete component scope rerun. Remaining
integration consumers and existing lint findings still require work.

The first compile exposed a Darwin ioctl argument type mismatch in the new PTY
test, corrected with the native argument conversion. The next run is explicitly
nonpassing: the new mode test waited for a helper before draining its PTY, and
its own library test process was terminated after a sampled wait4 cleanup stall.
No missing result is relabeled as a pass. The corrected harness consumes Command,
closes parent slave handles, drains while the helper lives and requires actual
successful exit plus its marker. A focused rerun and both later complete runtime
runs pass. The start-lock control also observes the blocking caller's return
independently of parent async polling and cleans up the real child before its
negative assertions. Its short observation is not a startup latency guarantee.

After the final checks, one ordinary line-comment annotation on the shared
upstream handoff restores the existing retained-ui marker count. Exact reversal
reproduces the tested file, with unchanged line count and all executable/config
bytes unchanged; no hook-loss exception is used. Final annotated 23-file patch
SHA-256 is `4cb7cf73708b385adc44354d5590ad8645bc32d517ac12fb9e684e90996005e4`.
Only that annotation and this review follow the passing checks. Bounded combined
resource/policy reviews are clean within their stated scopes.

Actual App/outer-loop integration, the retained C0 backend/read host, original
physical IO retirement, genuine Native editor/C2 resource issuers, complete
App/BottomPane/frame state and certified package/activation remain outstanding.
Local resources, a successful test process or a Git publication prove none of
those requirements. The live-update goal remains active and incomplete.
