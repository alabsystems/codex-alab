<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Original App editor job and surviving read owners

The original App now owns the real editor job instead of tying its lifetime to
one borrowed wait. The actual paired receiver census is bound in the original
outer frame. A processed Launch publishes the original Command/TempPath,
pre-child input/mode resources and full or refused partial bundle before the
same original worker starts. Queued Launch remains distinct from an accepted
job. Both the top-loop and immediate post-select pump run before ordinary
replay, reconnect, terminal maintenance and startup event drainage.

Canceled observation retains the same child, document read, callback and
resource owners. Normal App/backend/terminal event queues stay parked as under
the original inline editor policy. Local handback restores original stdio,
mode ownership, Tui policy and parser in that order, then resets the editor,
trims/applies the original result or original error and schedules the frame.
A genuine no-child cancellation uses the existing operation-bound receipt and
partial-mode rollback. Uncertain work retains its resources and real error.

The original read exposes Starting, RootLive, RootReapedReadPending and
FinishedPending observations without waiting for the original blocking open
or read to finish. Its actual File is published before the same read, with no
state mutex held through IO; observation can duplicate only that already-open
File. The actual spawn-completed owner requires the same accepted spawn worker
to be observed and joined before inherited stdio may be retired. These local
owners grant no Native READ domain, process role, foreground or NoFrontendIO.

One shared stdio census captures genuine FD0/1/2 and exact optional saved stderr
under the original stderr lock and native FD exclusion. CLOEXEC duplicates
start above the standard descriptors, so an absent stdin is not accidentally
filled by a preceding clone. A later refusal leaves originals unchanged. The
operation-specific capture correlates the actual Job, input and mode fences.
It still duplicates descriptions; it does not physically retire original IO.

Adversarial Root review found a race in the initial new stderr restriction:
checking editor ownership released the mode mutex before stderr changed.
The corrected opaque ParentStdioWriter holds the same original mode mutex
through all five full stderr mutation scopes. The existing real mode/PTY
control exercises actual FD mutation under that guard, verifies the same lock
is held, checks refusal under an actual editor and permits the writer again
after exact real handback. Independent review confirms the local lock order.
The App cut uses a single semaphore permit for cancellation-safe serialization,
matching the SDK progression design; no disallowed async mutex guard crosses
an await and no lint suppression is used.

Source-selected Trust 15698, through the protected qualified project controller,
ran the explicitly unverified component scope on base
`9c005ceecd69ed2443e49e8feccc92c590b4cada` plus the 23-file patch
`d413fca31fb0fae0bade741123ccf55a2322fe47277658289553004c51558db8`.
All 1,126 selected methods across thirteen libraries pass: zero failures and
three existing ignores. Every prior 1,123 passing identity is preserved, with
three new method identities for the actual App/PTY control, original read
matrix and shared stdio census. The existing mode/Child control also includes
the writer-lock regression. The strengthened App control captures actual C1
input/mode/stdio under the same cached cut, releases it locally without a
Native offer, then exercises original stdio restoration and once-only callback.
The ordinary queued Exit stays parked throughout.

Runtime log SHA-256:
`69e7cde3a2a5b9d4439e29b5c8791aa282fe5b6cf5f88786b5a058ade6eb4ae6`.
The same SDK owner bytes also passed all 352 SDK methods with four existing
ignores and its native-source-proof feature check. These are component scopes,
not all upstream tests or application certification.

Focused Tippy exits zero but reports 1,008 owned diagnostics, zero spanning,
failing-target or missing-script classifications and twenty upstream diagnostics
set aside. This is NOT lint clean and does not pass the shipping gate. New
async-guard/style findings were corrected without suppression. Remaining Native
consumers and prior owned findings still need integration and qualification.
Outside original-app-c0-native5 records retain exact input, controller/runner
identities, logs, statuses and method counts. Only this review follows the checks.

The first strengthened run reports all 1,126 methods passing but its controller
exits one when contained scratch cleanup reports a nonempty temporary plugin
clone. It is explicitly retained as nonpassing. A subsequent actual system
census finds no owned process left, policy-preserving cleanup succeeds, and
one identical-input rerun completes both maintained scopes with actual exit
zero. No missing or failed controller result is relabeled as a pass.

The first aggregate compile exits 101 before runtime: a separate native App
fixture lacked its new field, an App test wrote a private ChatWidget field,
and its quoting helper named an unavailable dependency. The native fixture
gets the marked default; an existing ALab widget hook supplies a test-only
actual sender binding without production privacy changes; the test uses the
existing shlex dependency. Failed records are retained as nonpassing.

Actual production retained-update observation still needs the Native C0
coordinator; this component keeps the original inline wait policy. Genuine
C0 READ enrollment, original physical IO retirement, signed same-PTY Native
helper/C2 child and foreground ownership, whole App/Pane/frame assembly and
certified package/activation remain incomplete. No Native permission,
complete scene, shipping qualification or seamless-update completion is claimed.
The active goal remains unchanged.
