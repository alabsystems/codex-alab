<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->

# Credit fallback with exhausted plan allowance

The managed macOS build 2026111701 received a positive credit balance from the
loopback provider after its second model request hit the weekly limit. It
printed the empty-reset inventory and waiting notices, made no third model
request, and reached the 120-second guardian timeout. This run used fixture
credentials. Its executable SHA-256 was
`090aacf5157ce4326da1744ab8869218dff31dc4fb7bde1f9bd226736f7022b3`.

The original smoke check also rejected a successful paid retry because it
required a later reading showing restored plan allowance. A replay of the valid
fixture trace reproduced that failure on the parent of `ee4e99f95f`. The
corrected package gate checks the latest authenticated credit reading before
the retry while the plan stays exhausted. It requires one retry of the original
input with each completed tool output present once and zero reset redemptions.
The regressions also exercise withdrawn credit access and duplicate tool output.

The Python smoke, provider-fixture, reset-notice and runtime-controller suites
passed 250 tests on managed Python 3.14. The weekly smoke and notice suites
also passed 64 tests on the system Python. The native gate on `715c2e5bbc`
finished linting with no diagnostic on the changed credit paths. Existing
warnings elsewhere in the ALab layer, including unused retained-UI code,
stopped the gate before certification. Running the smoke on a certified
executable that includes the changes remains pending.

## Empty-reset transcript

The `no-credit` probe in `95111e762e` passed managed build `2026111701` even
though the CLI printed three reset notices. Those included the zero-available
inventory and the message about waiting for allowance or another account.
The revised probe rejected the same executable. Each loopback run made two
model requests, read two empty inventories 30 seconds apart and redeemed
zero resets. The failure was the transcript.

An empty inventory calls for no reset action, so the package check now requires
that these polls produce zero reset notices. The native quota-wait message is
still permitted. A separate funded-account case requires exactly one notice
when recovery retries the request using available credits.

## Packaged executable check

The packaged-binary run against signed build `2026111701` read source
`70d63912591840da2a154f057dc0cade90a17d79` from its build record and measured
the executable digest recorded above. The quiet-wait case reproduced the reset
notices. For packages that carry compiler verification, the probe now checks
the observed executable against both recorded hashes and checks the source
before starting a case. This ties the transcript to the intended executable.

A separate `alab/package.sh --no-promote` run targeted the exact credit
source `d48d0d9757925632638760d81938282d899de48b` with its locked Trust
15698 compiler. The initial compiler check completed. CPU admission kept the
contained identity probe queued for over 16 minutes. A retry with the newer
native publisher and a subsequent one-core request met the same condition.
The queued probes were stopped. The application build and its certified-package
runtime checks remain pending until the contained probe receives capacity.

## Inspector build on November 19

Source `85bb9d8c8a` passed 329 targeted native TUI tests, including first
billing reads, unavailable reports and a reply arriving after a chat switch.
The protected package command selected the locked Trust 15698 compiler and
identified candidate `0.12.0-dev.21496+codex.0.169.1.g85bb9d8c8`. With one
build job, the contained compiler probe was still waiting for CPU admission
after two minutes. It was canceled before the application build started.

The host reported normal memory pressure and about 98 GiB available. Its native
CPU lease inspection reported an absent pool. A separate launch requesting one
CPU unit and 32 GiB through `aterm resource-run` exited 75 before submission
because retained host custody was unavailable. These readings establish the
admission result. The reason for the unavailable grant remains unresolved.

Resume qualification when the host grants the contained probe. Run the package
smokes for `existing-usage-credits` and `no-credit` against the resulting
executable and its matching source record. Signed build 2026111701 remains the
installed comparison.

## Plan renewal during the funded retry check

With an empty earned-reset list and a positive balance, signed build
`2026111701` printed five reset notices and resumed the retained request after
its next 30-second poll. The loopback provider restored ordinary allowance
during the list read. The package gate now requires the client to observe that
change before its next scheduled poll, retain the completed work, and emit one
ordinary restoration notice. The still-exhausted case continues to require the
paid-retry notice. Both cases exercise the runtime without billing an account.

## Release gate coverage

The standalone quiet-wait probe was available but absent from the gate's
selected cases. Running the updated controller against signed build
`2026111701` rejected its three reset notices after two empty listings 30.01
seconds apart and zero redemptions. The two funded cases also require their
single decision notice within the original turn, before its recovered answer.
A transcript replay previously passed all six placements before the turn, after
the answer, and after completion across the paid and ordinary recovery cases.

The protected CPU pool still reported `absent`. The one-unit admission probe
exited 75 before submission. Candidate package execution remains pending.

## Individual monthly credit limit

With a reported balance of 12.50 and an individual limit of 0% remaining,
the account inspector displayed `12.50 credits left` when the used amount
was unreadable. Paid recovery already checked the percentage independently.
The inspector now retains that percentage, the reported reset time, and the
balance when either amount in the monthly limit is unreadable. A later 40%
reading restores the spendable balance display. A stale or removed limit
leaves the monthly line out of the inspector.

At `b28f8f6e2c`, 185 native status and account tests and 528 ALab TUI tests
passed in the unverified test lane, with six ignored. The protected resource
probe returned 75 before submission. Signed package verification is pending.

At the monthly boundary, the rendered baseline still reported spending blocked
and 0% remaining. Source `76429d924e` scheduled a frame at that boundary,
marked the monthly figure for refresh, kept the last reported balance, and
showed the new percentage on the next account reading. A monthly limit
reported without a balance kept the balance slot empty. All 299 selected
interface, account, allowance and status tests passed in the unverified lane.
The protected release admission probe still exited 75 before submission.

## Balance in the paid-retry notice

The near-reset fixture reported 30966.04065 credits. The longer-window fixture
began with that balance and changed it to 30965.02000 while recovery listed
earned resets. The native retry notice used the authorizing reading in each
case and retained its precision. Replaying the refused retry produced no
further paid notice or redemption. Unlimited and unreadable balances also
have distinct notice coverage. The balance describes the account before
retrying; billing attribution comes from the chat report.

Source `27c1acd584` passed all 105 native quota-recovery tests and 142 release
controller, weekly smoke, notice and provider-fixture tests. The protected
release admission probe exited 75 before submission.

## Unavailable and mixed-source chat reports

A billing check can fail to confirm the current personal account after a login
change or when account discovery disagrees with the quota reading. Discard the
old chat report and show `Chat spending unavailable` in the inspector until
the next eligible read. A successful query with no chat row still means
`Chat spending unreported`. Keep those messages local to the inspector so an
unsuccessful check does not add another warning to the transcript.

When the provider supplies both a net amount and the source `plan_and_credits`,
retain both in the spending line. Lead with the debit, zero net amount, or
refund so narrow footers retain the figure. Attribute plan use only when the
provider explicitly reports that source.

Before the change, three native regression checks reproduced the missing
mixed-source label, a blank spending line after plan discovery failed, and a
missing status after a sign-in change. With the change, all 129 selected
native account and allowance tests passed. A read-only personal billing
query also returned a partial report with a credit source, an exact decimal
debit, and a provider cutoff earlier than the account balance reading.
The protected CPU admission probe exited 75 before submission.

At `260834011d`, all 105 quota-recovery tests, 528 ALab TUI tests, and
57 weekly-reset smoke harness tests also passed. Six TUI tests were ignored.
The next package qualification gap is a refused paid retry while the account
remains funded. Native recovery covers the refusal. A packaged test should
observe the retained request through a second paced poll and verify that only
the first retry emitted a credit notice.

## Refused paid retry

A funded account can still receive a second provider quota refusal. Include
that case in the packaged executable gate because a successful retry cannot
expose repeated sampling or notices while the original turn waits. The
loopback service keeps the account funded and its included allowance denied.
The gate checks the authorizing balance, exact retained input, a single paid
notice, zero redemptions, and a paced follow-up before interrupting the turn.

Signed build 2026111701 never made the funded retry. It sent two model
requests, listed the empty earned-reset inventory twice about 30 seconds
apart and printed three reset notices. Its JSON renderer finalized the plan
display during cancellation. Check tool completion in the retry request and
the final plan transcript independently of that renderer timing.

Three account reads spaced 15 seconds apart initially passed the pacing check.
Require a full polling interval (29 seconds with timing tolerance) between
consecutive reads. The release probe now distinguishes a missing model retry
from a missing paced poll. Against signed build 2026111701 it recorded two of
the required three model requests. On that same executable the existing
`stale-restored` control passed with consecutive usage reads 30 seconds apart.

The managed Python 3.14 run passed 58 weekly smoke tests, 30 runtime controller
tests, 13 reset-notice tests and 42 provider-fixture tests. The system Python
3.9 cannot run the controller's existing strict zip checks. Candidate package
execution remains pending because the protected CPU grant was unavailable
(exit 75 before submission). Resume with the new `paid-retry-refused` case
alongside `existing-usage-credits` and `no-credit`.

## Spending blocked and reopened

A native preflight with a positive balance and a reached spending control
printed a warning about account identity and an unconfirmed redemption. Its
ledger was empty. Treat an authenticated, explicit account restriction as a
completed passive policy check. The account inspector already shows the block.
The check still requires a known ordinary permission, matching account and
user, and the existing ledger checks. A different account still produces a
verification failure.

The native recovery test kept the same turn and balance through a block
reported just after the first empty reset listing. Recovery waited quietly.
After the provider lifted the restriction it released one paid retry, then
refused a duplicate. There was one notice and no redemption POST.

The packaged executable gate now exercises this sequence as
`spend-control-reopens`. The original request must complete with its tool
output and the balance from the final authorization. The probe checks that
the denied reading preceded a paced poll while the balance remained positive.
Managed build 2026111701 sent two model requests, saw the spending restriction
on its later poll and reached the 120-second timeout without a paid retry.

All 106 native quota-recovery tests and 144 managed Python smoke, controller,
provider and notice tests passed. The protected CPU probe exited 75 before
submission, so qualification on a new certified executable is still pending.

## Active spending restriction

After an ordinary model quota refusal, an authenticated usage read can give
a more specific reason that existing credits cannot fund the retry. The old
recovery path produced the same account-or-ledger warning for a confirmed
spending control as for an unverified reset. Report the account spending limit,
workspace credit depletion or workspace spending limit once during recovery.
Keep the existing verification path for an outstanding redemption. Native tests
exercise repeat reads and confirm that the pending operation retains its key.

Signed build 2026111701 saw the funding and spending block in its preflight
and after its model quota refusal. It printed four reset warnings and queried
the earned-reset inventory before the native wait. The new package case rejects
that lookup and requires exactly one restriction notice after the original tool
work and before the native wait. All 107 native recovery tests and 145 managed
Python smoke, notice, fixture and controller tests passed. The protected CPU
grant remained unavailable (exit 75 before submission).

## Workspace refusal on the model request

The provider fixtures for depleted workspace credits and a workspace spending
cap decode to native errors that already explain how an owner or member can
continue. Recovery also printed an earned-reset warning on those requests.
Let the native error carry the explanation when the refusal supplies an
explicit workspace reason. Keep the existing advisory for a nonordinary limit
whose refusal omits that reason.

On signed build 2026111701, a loopback replay of the member credit-depletion
fixture produced one extra reset warning immediately before the native quota
wait told the member to ask their workspace owner to refill. The capture ended
at its 22-second bound. The Core regression exercises the interactive and
headless recovery paths with request counters for usage and earned resets.
All 12 weekly compaction and recovery tests and 32 retry tests passed. The
retry suite runs its isolated transport test through the parent harness.

## Chat charges with an unreported account balance

The footer renderer reproduced a last-reported chat debit followed by
`resets 0` when the account balance was absent. A billing debit and an
account balance arrive independently. Either can justify giving the footer
space to paid usage and keeping a confirmed empty reset inventory in account
details. A stale or positive inventory still appears when space permits.

The renderer also discarded the weekly allowance when its detailed line was
too wide, even if the short provider reading fit beside the charge. Exercise
both choices with a spent weekly window and a cached debit at three widths,
including the exact fit and one column below. The protected CPU admission
probe exited 75 before submission.
The full ALab TUI library suite passed 529 tests. Its six executable-dependent
upgrade tests are ignored in this lane.

## Transcript labels for paid credits

The credit retry and spending restriction passed through the same formatter
as an earned-reset redemption. As a result the transcript labeled either
decision `[codex-alab reset]`. Label the decision at its source, where recovery
knows which resource it checked. Both the TUI and exec receive the same text.
The release check now counts each label separately and rejects a paid retry
carrying the earned-reset label. An empty poll must produce zero of both.

Verification passed 107 quota-recovery tests, 529 TUI tests, 14 Core tests
covering notice delivery and weekly recovery, and 103 smoke and release-check
tests. The six executable-dependent upgrade tests remained skipped. A fresh
protected build-slot request exited 75 before submission.

## Balance and chat reports refresh independently

A temporary billing failure or an older billing cutoff retains a chat's
previous report. On a narrow footer that amount could hide a current account
balance. Carry the retained status with the displayed report so the balance
can lead until billing responds again. The inspector keeps the charge and its
original cutoff. A partial report from a successful check still leads with
the chat charge.

When the account reading itself ages out, keep its last reported balance
with an explicit stale label. This preserves the provider's decimal precision,
including zero, without promising that the credits can be spent. Keep the
generic stale label when the provider omitted the amount.

Before the change, a 24-column native footer rendered only
`Last report · Partial ·…` with 25 credits available, and a stale account
reading replaced a reported zero with the generic stale label. After the
change, all 130 account and billing UI tests and 529 ALab TUI library tests
passed. Six executable-dependent upgrade tests were skipped. Protected build
admission returned 75 before submission.

## Credit availability after the last charge

The account display used the same label for an unavailable balance of zero
and an unavailable balance with no usable amount. A narrow footer also put
an earlier chat charge ahead of either reading. Show the unavailable account
first, using the provider's amount when present. Normalize signed and
exponent forms of zero to `0 credits left`. Keep a missing amount explicit.
The account inspector continues to show the chat's charge independently.

The focused native suites passed 175 chat widget tests and 80 interface tests,
including loss and restoration of spending access after a paid chat charge.

## Spending restrictions in the account display

The provider can report both a positive balance and a reason the account
cannot spend it. The inspector previously collapsed four workspace reasons
into one generic message. Use the same meaning as the recovery notice: show
workspace depletion, a workspace spending limit or the member’s spending
limit next to the amount. A provider-specified workspace reason takes
precedence over its generic account spending flag. The monthly limit only
applies until its reset time. A sparse update retains the reason, and a fresh
account reading can clear it. This uses the existing account read and emits
no additional transcript messages.

After the restriction change, all 175 selected native chat widget tests and
80 interface tests passed. The standalone ALab TUI suite passed 529 tests,
including the 25-column workspace depletion display. Six upgrade tests
require a separate CLI executable and were ignored. Protected build
admission returned 75 before submission. The managed binary is 2026111701.

## Expiring earned resets beside paid credits

The footer could fit a credit balance and an expiration notice, yet shortening
the weekly meter dropped the notice. Preserve the balance and give a fresh
expiration notice the remaining space. Include the short weekly reading when
both fit. Use a lower bound when the provider supplies only some expiration
dates. A stale inventory cannot support a current expiration claim. The
change uses the existing reset snapshot and produces no transcript output.

At 38 columns the original renderer showed `25 credits left · weekly 75% used`.
The regression now shows `25 credits left · 1 reset expires soon` at that
width, keeps both readings when they fit, and omits stale warnings. All 530
ALab TUI tests passed; six executable-dependent upgrade tests were ignored.
The protected release host returned 75 before submission.

## Empty inventories and reset review directions

When a count-only account response confirmed zero available resets, the
inspector also said their expiration was unavailable. Show the zero count
alone. Keep missing dates visible for available resets and expired counts
visible when all known resets have elapsed. When both expiring and expired
resets are present, show the counts together with one direction to review
resets in `/usage`. These changes affect the inspector only.

All 176 selected native interface and billing tests passed. The inspector
regression covers a count-only zero, two available resets with missing dates,
an expired last reset, and a mix of expired and soon-to-expire resets. All 14
transcript reset-notice checks also passed.

## Polling a confirmed empty reset inventory

Routine account polls were requesting reset expiration rows every 15 minutes
even when intervening reads continued to confirm an empty inventory. Use the
timestamp of the provider’s reset count to select count-only polling for a
fresh zero. The account and paid balance still refresh. An explicit inspector
check can request details immediately. A positive count clears the old detail
attempt and makes the next existing poll eligible.

The footer already deduplicates available Codex resets and limits them to
the reported count. Use those same rows when scheduling redraws and deciding
whether a count-only reply invalidates cached expiration data. A duplicate
or out-of-count row could previously discard a valid deadline and trigger
another lookup. With zero resets, only the count becoming stale can change
the display.

With managed app-server 2026111701 and a funded loopback fixture, two
count-only reads each returned the paid balance and zero resets using one
usage request and zero reset-detail requests. A read explicitly including
details made one of each request. This verifies the transport flag separately
from the new TUI polling decision. All 31 quota-wait, reset-notice and reset
command-status checks passed with the managed Python 3.14 runtime.

All 178 selected native allowance and interface tests passed, including
explicit inspection after zero, clock rollback, a newly earned reset,
duplicate detail IDs, and expiry rows beyond the provider count. All 59
weekly-reset fixture checks passed. The protected host returned 75 before
signed-build submission.

## Repeated notices for a single earned reset

The runtime suppresses unchanged notices, but the release check accepted a
second identical inventory, decision, application, or restoration during a
single redemption. It also accepted a repeated preservation decision. Reject
these repetitions in the representative single-reset fixtures. Consecutive
backend reads can agree while the transcript stays quiet. A changed inventory
can be printed, including a return to an earlier inventory after an intervening
change, when a subsequent listing supports it.

The previous check accepted all 34 injected repetitions across ten fixture
cases. All 90 weekly-reset, notice, quota-wait and command-status checks pass
with the stricter rule. In loopback runs, managed build 2026111701 passed the
startup and mid-turn redemption cases. Each made one redemption request.

## Notice placement in a recovered turn

The earned-reset qualification previously accepted a correct message sequence
with the inventory before the turn started or the restoration after the
answer or turn completion. All three misplaced transcripts passed the old
check. For a successful representative recovery, compare every reset or
paid-credit notice with the notices between the original turn start and its
answer. Leave startup diagnostics and other unrelated output independent.

All 91 weekly-reset, notice, quota-wait and command-status tests passed. The
managed executable 2026111701 also passed all ten selected loopback cases.
Each of the eight startup or mid-turn redemptions consumed one earned reset.
Both 24-hour preservation cases consumed zero. Signed-build admission
returned 75 before submission.

## Weekly time in earned-reset decisions

The release check accepted a five-hour reset time, a missing time or the word
“tomorrow” in both the redemption and preservation notices. Each selected
fixture supplies a weekly refresh time. Match the printed UTC time to the
eligible weekly allowance from the usage response preceding the displayed
inventory. A later response cannot justify an earlier notice. Either window
position may carry the weekly limit.

All six incorrect notices passed the old check and fail the new one. All 93
weekly-reset, notice, quota-wait and command-status tests passed. Managed
build 2026111701 passed all ten representative loopback cases, including both
24-hour preservation cases. Replaying their saved transcripts reproduced
the result.

## Evidence for a quiet wait without credits

The quiet-wait fixture established an empty earned-reset inventory but omitted
paid credit availability. Its verdict could accept a silent wait even with a
positive spendable balance. Have the fixture explicitly report zero paid
credits. Require a usage read after the original quota refusal and before the
first reset listing. The plan must remain exhausted, and every subsequent
usage read must still report zero paid balance with spending and unlimited
access disabled. Two empty post-refusal lists witness repeated checks.

All 94 weekly-reset, notice, quota-wait and command-status tests passed.
Managed build 2026111701 failed this loopback case at the transcript check.
It printed three reset warnings, including both reported in tab 2. Replaying
the captured traffic with these three messages removed passed. Changing the
paid balance in that quiet replay to 25 spendable credits failed the funding
check. Signed-build admission returned 75 before submission.

## Conflicting spending signals during a paid retry

The package check accepted eight altered readings across two stages: the
paced check that says spending reopened and the final check authorizing the
retry. Each kept the generic spending flag clear but reported workspace
depletion, a workspace cap, an exhausted individual limit, or an upsell.
The native recovery policy already checks these signals. Require the package
evidence to agree before accepting a credit-backed retry. A general plan
rate limit and a positive individual allowance still permit the funded case.

All 59 weekly-reset tests and 35 notice, quota-wait and command-status tests
passed. The valid reopening case preserved the completed tool result, retried
once, emitted one credit notice and made no earned-reset redemption.

## Ownership of the balance behind a credit decision

The package fixture authenticated the outgoing requests, but its paid-retry
check could accept an unrelated account or user ID in the usage response.
The native recovery code matches both IDs to the active login. Apply that
same check to the funded fixtures and the unfunded quiet-wait fixture.

All four public fixture runs with a missing or substituted owner previously
qualified. They now fail before writing a successful case result. The
blocked-spending and quiet-wait checks also reject a mismatched owner.
Replaying a captured quiet-wait trace with the old reset warnings removed
passed with its original owner. Substituting a different response user
failed even though the recorded request headers authenticated correctly.

All 60 weekly-reset tests, 35 quota-wait, notice and status tests, 30 runtime
controller tests and 107 native quota-recovery tests passed. The native cases
confirmed that a spending restriction can clear for the same user and that an
account change cannot release the previous user's retry. The managed package
remained at 2026111701. Signed-build admission returned 75 before submission.
