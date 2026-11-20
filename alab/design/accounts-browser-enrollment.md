<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Registry-driven browser sign-in for Codex subscriptions

**Design decision.** aterm resolves the selected subscription from the Registry
before opening the browser. It looks up the delivery mailbox in advance and
requires a proven route before requesting or reading an email code. A flow
that completes using a known browser session or a passkey can finish without
accessing mail.
The account gets its own browser session. A provider account chooser is useful
inside that session. Codex shows progress and the identity that actually
connected. Ordinary token renewal and switching between already-enrolled
slots continue to use their existing native paths.

## Resolve the intended account

The native host resolves a local slot to one Registry account key. It captures
the following inputs together under the existing enrollment operation.

| Input | Use |
|---|---|
| Registry account key, provider, sign-in method and login address | Choose the provider flow and the address entered on the sign-in page. |
| Expected provider identity, falling back to the login address, and any established provider account/workspace binding | Verify the credential returned to the staged slot. The receiving mailbox's own address is a separate identity. |
| Login address's receiving mailbox key, routing evidence and alias chain | Select the mail connector allowed to read this attempt's verification message. Two distinct subscription keys can use the same mailbox. |
| Local slot, profile owner, and revisions of the binding and Registry inputs | Keep browser storage and credential writes scoped to the selected account. Detect a reassigned slot, a changed route, or a new hold before publication. |

Use the native `Lab::account_of` join and the Registry's effective mail route.
An explicit owner binding, including one reached through a declared alias, or
a unique provider-observed delivery association can drive unattended retrieval.
An exact rule can also qualify if its metadata records the owner's approval
for this address and mailbox. A domain wildcard, a guessed inbox, or equally
ranked conflicting mailboxes cannot establish ownership. The host needs the route's evidence and
ambiguity status alongside its mailbox key. Signed aterm 0.208.0 reduces
that information to an address-to-login map. The local native candidate
`417c6744fdd` also carries the stable mailbox key, the mailbox login when
known, the owner or provider evidence, and the address keys followed through
the alias chain. It excludes routes supported only by a routing rule or hint.

The native mail summary can show one provisional mailbox while Registry
writers still disagree about an address. In candidate `417c6744fdd`, two
reads can return that same choice throughout an enrollment. Before opening
the mailbox, verify the typed Registry fields that determine the route,
including address protection, the owner mailbox and any alias followed.
An explicit, settled owner decides the route before an alias is consulted.
When a field that decides the route is disputed, wait for it to settle.
Reconfirm the route before submitting the code.

If the account record is incomplete, show `Waiting for Registry account`.
If email verification is required and its route is unresolved, show
`Waiting for Registry mailbox route`. Request the relevant data through
normal Registry synchronization and retry when those records change. Reuse an
admissible local snapshot under the host's existing freshness and hold rules. An empty
incomplete snapshot cannot authorize a new binding. The existing signed-in
slots remain available during this wait. The native connector resolves secret
references locally. The TUI receives display status and an operation ID.
[Registry account view](https://github.com/alabsystems/aterm/blob/7a9ff69000343b017cf1e492eaa807d99f41cb90/crates/aterm-agent/src/harness/alab_roster.rs) · [Mail route evidence](https://github.com/alabsystems/aterm/blob/7a9ff69000343b017cf1e492eaa807d99f41cb90/crates/aterm-db/src/alab/mail.rs)

## Browser and email flow

1. **Open the intended account's session.** Reuse a native profile with proven
   ownership of this account. On first enrollment or after a profile mismatch,
   create a fresh browser data store dedicated to this attempt. Give each
   simultaneous enrollment its own storage. Opening another window in the
   user's everyday profile does not establish isolation.
2. **Handle the provider's page.** On a recognized provider account chooser,
   select the unique entry matching the Registry's expected provider identity.
   For a submit button, check the email shown against the identity fields its
   form would submit, including fields linked from outside the form. Other
   buttons in the same form can belong to different accounts. Recheck the
   selected button, its account fields and its destination before clicking.
   Use that identity again on consent. The login form and verification email
   both use the registered sign-in address, which can be an alias. If the target is
   absent or the rows are ambiguous, use the provider's *Use another account*
   path and enter the Registry login address. If a remembered session silently
   selects the wrong account, discard that attempt and start once in clean
   storage. Only positive evidence of a wrong identity triggers this retry.
   Use the registered sign-in method. A provider-required approval or
   verification challenge stays attached to its original attempt and remains
   visible with its actual status.
3. **Retrieve the matching code.** When the provider requests email
   verification, the host opens the resolved delivery mailbox using the native
   connector, verifies the mailbox identity, and accepts a fresh message from
   the expected sender for this login attempt. Apply the host's current access
   policy to both the requested account and the receiving inbox, including a
   protected address reached through a shared mailbox or a tagged address.
   Recheck that policy and the selected route after the read. Record the
   connector as working only after both checks pass. Match the envelope recipient or
   provider correlation when available. Where two aliases share an inbox,
   capture the mailbox cursor before requesting a code and serialize challenges
   that have no distinguishing transaction or recipient field. Hold the
   mailbox reservation through consumption or expiry. An earlier request's
   mail can arrive late, even after expiry. If it could belong to either
   request, keep the attempt pending for uniquely attributable evidence. Enter
   an eligible code, or follow a verification link, only from the provider
   page that requested the mail. The mailbox read can outlast that page.
   Check the document, visible verification prompt and any displayed
   recipient again when using the response. A delivery notice naming an
   address must match the registered login alias, including when the
   chooser used a different canonical identity. A field that appears late on
   the original prompt can accept the code. A different recipient, a new
   document at the same URL, or a provider approval screen ends that use.
   Verify that the control submitting the code sends it to an approved
   provider destination before filling it.
   Retain the selected mailbox binding until submission. If its route or
   access policy changes while the form finishes rendering, discard the code
   or link before sending the browser command. The headless reader uses the
   Registry for delivery and leaves publication of unrelated discovered
   mailbox addresses to the ordinary discovery flow.
   Keep codes and verification links out of the TUI, logs and receipts.
4. **Verify and publish.** Use the native staged OAuth callback or device
   transaction associated with this attempt. Verify the returned subscription
   identity and any established provider account binding. Recheck the slot
   and Registry revisions and atomically publish to the selected credential
   file. A different identity or changed binding ends that attempt without
   replacing either live login. Clean up the attempt's browser and callback
   resources. Two slots can stay signed in even when the current host admits
   just one browser enrollment at a time.

### Recover from a confirmed wrong account

Select fresh owned storage at admission when profile ownership is unproven,
before asking the provider for a device code or callback. The native
operation leases its profile and has a single-use grant exchange. Reopening
that operation must reattach to the same attempt.

For a retry, distinguish a positively identified wrong principal from an
ambiguous page, a provider verification hold and an uncertain exchange. If
the staged credential confirms a different principal, the exchange has
already run. Retire the predecessor and its callback before admitting a
successor with fresh browser storage, OAuth state and a new callback. Check
current Registry bindings and holds at admission. Deliver late callbacks
only to the attempt that owns them. A provider verification hold remains
attached to its attempt.

### Identify the request that sent the email

Record the provider action that requested the code. For a known send action,
capture the mailbox cursor immediately before dispatch and attach it to the
enrollment attempt when the browser confirms the action. A message delivered
after dispatch but before the code field appears can pass the freshness check.
Exclude messages already in the inbox before dispatch.

If the browser reopens on a code field, reuse the pending attempt's recorded
request and mailbox cursor. A provider transaction ID present in both the
challenge and the message can also identify the request. When both are
missing, the host needs an identifiable new request or must keep waiting.
The exact recipient distinguishes two different aliases that share an inbox.
Two successive requests for the same alias can still need a provider
transaction ID to distinguish a delayed message.

Persist the possibility of a send before dispatch. A cancellation, lost
browser acknowledgment or process restart leaves that record attached to
the original attempt. If a later message fits two requests for the same
alias, keep waiting for evidence that identifies which request produced
it. A mailbox delivery ID can identify a message across reads while still
leaving its request unknown. An already signed-in browser can finish through
the matching provider account without consulting mail.

The provider may omit the email from a consent page. Positive evidence from
the completed native exchange is required before a slot can be marked signed
in. The local UI can display the Registry's expected account during enrollment.
Display the connected email after a fresh slot-scoped `account/read` confirms
the identity and an authenticated `account/rateLimits/read` confirms the account.
Show the resulting allowance separately, so an authenticated account at its
weekly limit still reads as signed in. [Native browser profiles and consent handling](https://github.com/alabsystems/aterm/blob/7a9ff69000343b017cf1e492eaa807d99f41cb90/crates/aterm-agent/src/harness/headless_oauth.rs) · [Native credential admission](https://github.com/alabsystems/aterm/blob/7a9ff69000343b017cf1e492eaa807d99f41cb90/crates/aterm-agent/src/harness/cli_codex_enrollment_recovery.rs)

## Implementation and acceptance

Implement the evidence-preserving route lookup, chooser handling and bounded
clean-profile retry in aterm's existing enrollment driver. Add typed waiting
and verification states to the existing host operation where missing. Keep the
Codex side limited to selecting the slot, starting or reattaching to that
operation, and displaying its status. [Enrollment operations](accounts-enrollment.md)

| Trial | Required observation |
|---|---|
| The normal browser is signed in as A. Enroll B with and without a chooser. | The staged credential belongs to B. A's original browser and credential still use A. |
| The Registry gives a login alias and a different canonical provider email. | The login form and code recipient use the alias. The chooser, consent and staged credential match the canonical identity. Changing that binding during the challenge stops submission. |
| The chooser exposes two similar emails, silently returns A, or displays no email on consent. | A displayed label alone never publishes B. The first mismatch can recover in fresh storage. A second mismatch leaves both credentials intact. |
| The chooser lists A and B as submit buttons. A form field changes after B is selected. | A stable B submits once. If B's form now names A, the chooser receives no click. |
| Two aliases deliver to one verified inbox. Codes arrive in reverse order, or A expires and its uncorrelated message first appears after B requests a code. | Each transaction accepts only its own eligible message. The delayed ambiguous message is never submitted for B. |
| A prior message for B arrives during browser startup. B then requests a new code, which arrives before the code field renders. | The prior message is excluded by the request's cursor. The new message can be accepted with the matching sender, recipient and request evidence. |
| A new browser drive opens on a code prompt left over from a canceled sign-in. | Without evidence linking this drive to the send, it leaves the inbox and prompt untouched. A separate drive that reaches consent with the correct signed-in account can finish without mail. |
| B's verification page changes during the mailbox read or immediately before the browser acts. | A replacement document at the same URL, a different displayed recipient or a provider approval screen receives no code and follows no mail link. The original prompt can finish rendering normally. |
| The Registry is unavailable, two mailboxes claim the alias, or the slot changes during login. | The UX names the actual wait or mismatch. Neither mailbox guessing nor an obsolete callback can change a slot. |
| A disputed owner, alias or protection flag consistently projects the same provisional mailbox for A. B has an independent, settled route. | A waits before requesting or reading an email code. B can continue using its own route, including when both accounts ultimately share an inbox. |
| An unprotected login delivers mail to a protected inbox, or the inbox becomes protected during a read. | The host declines the first read. In the second case it discards the result before code submission or recording a successful connector. |
| Both subscriptions are enrolled. One has exhausted its weekly allowance. | Both live account reads return their own identities. Browser-free renewal preserves both slots. Switching to the usable slot resumes the same conversation. |

On November 18, 2026, Chrome for Testing 155.0.8059.84 exercised the isolation
and chooser cases on one synthetic local origin. With A signed in to the
fixture, a fresh profile had no A session. Asking for B while the chooser
offered only A produced no submission. After selecting B from a matching
chooser, both open profiles still returned their own accounts. This trial
used disposable profiles and no provider credentials. Provider-page handling
and the Registry-to-mailbox flow still need end-to-end qualification.

On November 19, Chrome 155 passed 17 offline checks against the browser
code in native candidate `a7ef68dc614`. Choosing B submitted B exactly once
with separate forms and with two buttons in a shared form. Changing the
selected identity or the submission method between the read and the click
produced zero clicks and zero submissions. The fixture supplied the selected
index and used synthetic accounts.

On November 19, the local aterm candidate `ecd045aa360` exercised the
login-alias and canonical-identity case against a synthetic browser and a
Registry-bound mailbox. Nine managed Codex tests passed, including two
aliases sharing one inbox and a Registry change before code submission.
The signed host still awaits these changes. Shared delivery alone does not
establish a subscription's canonical provider identity.

The local aterm candidate `58c8c081cd` passed all 11 managed enrollment tests
on November 19. In the browser fixture, one mailbox read sufficed when the
code field appeared late or stayed visible after submission. The browser
then reached consent and the callback. In the mail fixture, two copies of the
same code in one message remained eligible. Two distinct codes in one message
caused the reader to skip it, even when one code had already been used. These
checks can be repeated in the aterm checkout with
`targo --unverified test -p aterm-agent --lib managed_codex_`.

On the same Mac, the local native candidate `58199053c16` rejected a delayed
message whose delivery time was recent but whose send time preceded the
attempt. The earlier browser fixture submitted its code for the wrong
request. The corrected reader checks for an earlier message date in both
the mailbox listing and the complete message before using its code. A fresh
delivery time alone can no longer override an explicit older send time.

On November 19, native candidate `c4ac7e5a68b` passed 18 managed enrollment
tests. A delivery notice naming the canonical identity instead of the
registered alias stopped both code entry and mail-link navigation. This held
whether the notice appeared before or during the mailbox read. A separate
loopback Chromium trial passed 25 checks, including an actual reload at
the same URL and a code field that appeared late on the original page.
These trials establish continuity of the observed prompt. Validating every
possible submission destination of the original code form remains open.

Native candidate `07ebc6d5f84` checks the declared destination of the
code form and its submit controls against the host's approved HTTPS
destinations. A Chromium fixture gave the earlier code a form that always
pointed elsewhere. It edited and submitted that form. The new code stopped
before editing. The standalone Rust emitter passed 13 checks, and its
generated browser code passed 30 checks in loopback Chromium. These trials
cover declared destinations and changes during input. Page-authored
network requests after dispatch remain outside their scope. The full
native test build stopped for lack of disk space.

Native candidate `84cf1c3cba8` requires the current browser drive to observe
one uncanceled native submit event for the registered login address before
it can read mail. The event must come from the checked email form and its
selected submit button. A recognized submit event establishes that the
browser acted. Provider issuance still needs separate evidence. The
isolated native state and action checks passed 35 assertions. In six
scripted runs of the native driver, an inherited prompt and a lost or
unrecognized acknowledgment made zero mailbox reads. A remembered session
finished through consent without mail. A fresh acknowledged send followed
by a new code prompt read mail once. In 14 loopback HTTPS Chromium cases
the ordinary form sent the exact address,
while an inherited code form, a canceled submission and a hidden resend
action gave no acknowledgment. In another case a submit handler changed
the address. The fixture server received a request even though the browser
guard withheld permission to read mail. Retrying that action could send a
second request. This candidate keeps its acknowledgment state in memory
for one browser drive. The full native build remains pending.

Native candidate `417c6744fdd` records the local time immediately before the
browser dispatches the email form and passes that time to the mail reader
only after the browser acknowledges the submission. In the synthetic mail
trial, an earlier build accepted a message sent during browser startup,
one second before dispatch. The candidate rejected it. A message dated
in the dispatch second or delivered while the browser acknowledgment was
pending remained eligible. Recording a time only when the acknowledgment
returns would lose that legitimate delivery.

A one-second cutoff still leaves room for two sends to the same alias.
An offline trial at native commit `ceb7b969568` reproduced the ambiguity.
Two simulated requests for one alias began in the same second. The first
request's unseen message arrived while the second was waiting. The production
mail filter returned the first code. Changing only the mailbox delivery ID
gave the same result. Wrong-recipient, wrong-sender, older-date and replay
controls all rejected the message. No live mail or provider exchange took
part. Repeating the trial with the acknowledgment guard and the later
dispatch cutoff still admitted the delayed code when each browser drive
had separately submitted the same address. A host that cannot bind a delayed
delivery to its issuance must retain that uncertainty across cancellation
and restart.
