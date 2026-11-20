# Aligning codex-alab with OpenAI Codex 0.170.0

## Delivery status on November 20 UTC

**The 0.170.0 CLI and its matching Code Mode host are available as a local review build. The installed signed macOS ARM64
package is ALab 0.11.0, based on OpenAI 0.169.0.** The tagged review sources include the five mainline changes from ALab's upstream base
to the release branch point and the four changes on OpenAI's release branch.
All nine intake decisions passed. OpenAI published stable **0.171.0** at
00:44 UTC on November 20. The requested alignment target is **0.170.0**.
At 07:00 UTC OpenAI main was at `b059ce11f9`, 482 commits past the
0.170 release branch point. OpenAI published the latest preview,
0.173.0-alpha.1, at 02:50 UTC. The refreshed upstream comparison has
SHA-256 `e0c9bce62ad8fe16699d32b0cc1f82a4c3478e65e07caf93fb10ab90e30cfd84`.
The earlier official-source and signed-package verification has SHA-256
`bb2f873d24060984202e398b81b51f1ad3551d6e7cd6ecc25b29210060aee99e`.
[Requested 0.170 release](https://github.com/openai/codex/releases/tag/rust-v0.170.0) · [Latest stable release](https://github.com/openai/codex/releases/tag/rust-v0.171.0) · [Latest preview](https://github.com/openai/codex/releases/tag/rust-v0.173.0-alpha.1)

The package run on the earlier `23a3833888` review selected Trust 15698 and stopped at the cold
build’s disk requirement, with 21 GiB reported free and 35 GiB required.
The compiler identity check completed. Application certification and
signing remain pending.

On the Mac used for this review, sign in with the second account, keep the
same local Codex home and run the `6a986f3188` review executable.

```sh
~/.ubd/codex-0.170-review-6a986f3188/bin/codex resume
```

The picker shows active conversations from all local logins and projects,
newest first. Choose the conversation and press **Enter**. On this binary,
B picked an exec session created by official 0.171.0, continued it in
its original project, and A opened the same thread again on signing back in.
A separate terminal test narrowed both the login and project filters, quit,
and launched plain resume again. Both logins and projects reappeared.
Browsing preserved both histories and sent zero model requests.

You can also run `codex resume "Release notes"` with an exact saved title.
B opened A’s uniquely named exec session from another project and used B’s
login for the next turn. With the same title on B’s editor conversation,
the CLI reported both matching IDs, sent no model request and left both
histories byte-identical. The same test passed after A’s row disappeared
from the local database and the CLI recovered it from the saved history.
The previous `23a3833888` build missed the unique exec title and sent the
ambiguous prompt to the editor conversation. The new executable and its
matching Code Mode host passed the native stdio test. It passed **18 of
18** direct comparisons with official 0.170.0 and **14 of 18** with
official 0.171.0. Combined evidence SHA-256
`9781231bf8c2b58289aaec70d7905363593f5421c949345e073de941d1bb1768`.
[Named-resume review source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-6a986f3188) · [Two-account regression](https://github.com/alabsystems/codex-alab/commit/a7b9ac5a0a24269b31640772ff3083b1424a12bd)

Title lookup also crossed a real 100-row server page. Among 101
similarly titled conversations, B’s editor session was row 100 and A’s
exec session was row 101. B could open A’s uniquely titled conversation.
After the editor session received the same exact title, the command
reported both IDs. Both lookups preserved all 101 rollouts and made zero
inference requests. Verification SHA-256
`a3d528a4e6dbd6d21fea5db9c75b46eb07a40182d81799c2a1261e9119e4919e`.
[Pagination regression](https://github.com/alabsystems/codex-alab/commit/512b23aa619d292e4e69891c7fa62b53e84bec1f)

**Release qualification.** Comparing `6a986f3188` with private main
`0d07274075` found newer fork hooks absent from the review. The next
source, `aa54207c18`, integrates those changes. It passed the normal
hook comparison and all nine upstream intake decisions against `0d07274075`.
Two independent readers accepted **3 of 78** scenarios on this source.
Seven protected stages ran 111 distinct Python tests for build and adoption
scenarios 1, 2 and 4. Both readers identified the same 75 pending scenarios.
The result has SHA-256
`71e1291124e24dca42dd63e1e3bd20bff06ad597f6173d3cff1ddfa465237067`.
Private main has since advanced to `6938a26ba7`, which carries 58 additional
shell-output hooks. A release cut against that later baseline must reconcile
them. The detailed 64-of-78 result below belongs to `23a3833888` on its
original private base.
[Reconciled source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-aa54207c18)

**Earlier terminal evidence on `23a3833888`.** In one of seven trials A
changed the directory after B had opened the picker. B selected A’s
thread and its next shell ran in the new directory. Two trials selected
the correct thread from 26 identical titles. Those preserved 25,767
bytes of 0.170 history and 26,006 bytes of 0.171 history, along with all
25 neighboring histories in each test. The exact CLI and its matching
Code Mode host also passed their native stdio check. Removing the host
produced the expected failure. These trials use isolated test logins
and loopback services. The combined terminal and CLI comparison has
SHA-256 `1e8e4a63a8c777d77e554d6dc8d05f61da50b5ffef1cc4660f9ac0bc1251b62a`.
[Unified review source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-23a3833888) · [Resume guide](../README.md#resuming-local-work-after-switching-accounts)

B also used the default picker to find A's oldest conversation in two
histories of 10,001 sessions. Searching took 2.28 seconds when A's
index row was missing and 2.24 seconds when its preview was blank.
Both recovered the exact thread. All 10,001 rollout files and every one
of the 43 fields on the other 10,000 index rows stayed intact. Browsing
made zero inference requests. The verified scale record has SHA-256
`52b7e425b1cc05ea20abd8a3813894fdc07548ed1ab7f45a19e719c597d41453`.

For an archived conversation, B opened **Status → Archived** in the
same picker. B continued A’s original thread in A’s project with all
25,698 prior bytes intact. When A still had a different conversation
open, B could read it. Pressing **R** while A held the writer left all
25,587 bytes intact and made no model request. After A closed, pressing
**R** again in the same terminal let B run a shell with B’s credentials
in the original project. The two terminal trials have verification record
SHA-256 `abbdd193be744660eb436410658a6508f6d0eee936d8d5ef78fc8416fb019721`.

### Vanilla comparison and release plan

The tagged review source includes WebRTC negotiation for terminal voice
calls, automatic refresh of authentication headers for OpenTelemetry HTTP
exporters, and experimental support for identically named MCP servers
in different environments. The release also includes oversized WebSocket fallback to
HTTP and fixes to MCP OAuth cancellation and Cloud conversation recovery.
[OpenAI 0.170.0 release notes](https://github.com/openai/codex/releases/tag/rust-v0.170.0)

| Work | Result |
| --- | --- |
| Compare the exact stable tag | All 18 direct CLI and protocol comparisons passed, covering 77 help pages, exec events, app-server schema and initialization, effective config, MCP and login. The named-resume binary matched all fields in the four tested provider catalogs. Across this build and both official releases, 27 isolated app servers returned 399 model rows over nine configurations. All 48 turns completed and the 72 inference requests went to the local test services. Live provider entitlements were outside this measurement. |
| Compare the latest stable, 0.171.0 | The same binary passed 14 of 18 comparisons. The four differences cover CLI help (including remote exec), startup files and effective config in two terminal environments. Both releases have the Sol/Astra catalog. In the provider trials, the official versions differed only in two tool descriptions. The fork retained its existing analytics, reasoning, instruction and invocation-ID differences. OpenAI 0.171 adds default workspace previews, composer undo/redo, remote exec, terminal pairing and custom MCP status lines. [0.171 release](https://github.com/openai/codex/releases/tag/rust-v0.171.0) |
| Preserve the ALab additions | The fork carries cross-account local resume, named-account assignment and renewal, aterm Autopilot, goal and process recovery, and model migration. The resume fixes and their terminal regressions have landed on private main. |
| Compare ongoing OpenAI development | Regular ALab main incorporates OpenAI through `ce3031018e`. A separate development candidate incorporates 427 of the 487 later commits. The remaining 60 include host routing of selected analytics events, replay of pending requests after resume, Guardian policy and model refreshes, retention of pinned history during compaction, and retries when executor discovery fails. Vanilla’s picker defaults to the current account’s view of local history. [Analytics routing](https://github.com/openai/codex/pull/62684) · [Pinned history](https://github.com/openai/codex/pull/62677) · [Executor discovery](https://github.com/openai/codex/pull/62679) · [Vanilla picker](https://github.com/openai/codex/blob/ea8ccc1d20ff65398e02490ac34f8c1012848625/codex-rs/tui/src/resume_picker.rs#L2633) |
| Qualify and ship | At source `23a3833888`, **64 of 78** required fork scenarios passed. Complete the 12 missing test procedures and run the two live-shell cases on a compatible protected host. Integrate a release-eligible source through regular upstream intake, obtain dependency-inclusive Trust certification, allocate the next available ALab release, sign it and repeat the two-account terminal test on the signed executable. |

The full provider rows for `6a986f3188` have SHA-256
`d68020e14f094eae89c45003a98956aa8de190f8624c44ee15e3019b4b460f3d`.
The field-by-field HTTP comparison has SHA-256
`bca5a2a427b9f1162d22e985603a9b1594fdf2856225895c9407f5a88ed71ded`.

Bare `codex resume` gives the user a choice across logins. Automatic
selection with `--last` retains upstream’s current-login behavior. The
0.170 review and the official 0.170.0 and 0.171.0 binaries each ran five
interactive turns in two projects with two logins sharing a local Codex
home. A owned the newest conversation in the launch project. With B signed
in, `--last --all` reopened B’s newer conversation in the other project,
and `--last` reopened B’s conversation in the launch project. Both
continuations used B’s bearer and workspace, appended to the original
conversation and preserved its working directory. All three trials used the
local app server with daemon autostart disabled. Independent verification
SHA-256 `441b726310220cc53936fa52088d65ac0b78e2e4bed4101b6a38ddab248b3094`.
[Reproduce the two-login test](../smoke/account_resume_last.py)

On the 0.170 review, B also quit the bare picker twice while it was
narrowed to the current login. The first exit had the current-directory
filter selected and the second had the repository filter selected. Both
fresh launches displayed **All** projects and **All local** logins before
the first keystroke. B saw both original exec conversations, including
A’s in a linked Git worktree. Browsing made zero inference requests and
left both saved histories byte-identical. Terminal verification SHA-256
`53d2b874081557149efd564a278f43b1656f853ed5d411f4ee759d99fa5504dd`.
[Reproduce the filter restart](../smoke/account_resume_filter_restart.py)

The default picker also covers editor conversations. A created a thread
through the local stdio app server in each of the review, official 0.170.0
and official 0.171.0 binaries. All three recorded the source as `vscode`.
B opened it from another project on the review build, ran a shell in A’s
saved directory with B’s credentials, and A returned to the same thread.
B also found the editor conversation on a first login with no sessions.
Official 0.171.0 showed zero rows when B opened its bare picker in A’s
original directory. An all-account listing on that same official daemon
returned A’s exact thread. The three-way audit has SHA-256
`43b74f446992e7466ba3a7df136b1e0511ca3f919d223e8751e018ca886db432`.
The official control has SHA-256
`548d48ebe7bac1c7e1423265e5f78da287e3f4e09cd5a6b3554de7e97c068652`.
[Editor-origin smoke](../smoke/account_resume.py)

One conversation created by a local API-key login and two created by
separate ChatGPT logins appeared together in the review picker. Narrowing
to **Current** under B showed B’s conversation and the API-key conversation.
B continued the API-key thread, reusing its shell result. The original
26,684 bytes, both neighboring histories and all creator records were
preserved. Independent terminal and request replay SHA-256
`e9b60957c4b462962e134b192707888c5de00979d5761d77ecfd1e717a23fd37`.
[API-key and ChatGPT history smoke](../smoke/account_resume_shared_history.py)

All three binaries also reopened a shared interactive thread with
`codex resume --last` for a first-time ChatGPT login. Its history contained
one API-key conversation and zero conversations created by B. Each CLI
sent exactly one continuation request as B on the original UUID, with the
original shell result. This is why the guide describes `--last` in terms
of conversations visible under **Current**. Three-binary verification
SHA-256 `7a6e067702a9c0ec85568c360b08796c86fe8b795e07a5c5e71ea6fc2e0277e8`.

For a historical upgrade test, the official macOS 0.164.0 release
created a genuine conversation under A. Its rollout included A’s user
and workspace IDs and omitted the later `account_scoped` marker. On the
0.170 review, B’s **Current** filter showed the 0.164 conversation and
B’s own conversation and excluded a newer thread created by A. B
continued the original UUID with B’s credentials and replayed the old
shell result. All 25,995 original bytes and both neighboring histories
were preserved. As a control, using a recent binary as the historical
producer failed on its recorded marker before the picker opened.
Verification SHA-256 `46184ab7ab45c69ed5120fac32272c624e19ae0e156a2a455f45b4cbb763db37`.
[OpenAI 0.164.0](https://github.com/openai/codex/releases/tag/rust-v0.164.0) · [Account-scoped history change](https://github.com/openai/codex/pull/58918)

<details>
<summary>How the two directory and history fixes were verified</summary>

On the previous `019cbd9f6b` build, one unreadable directory could hide
other, readable conversations that had yet to enter SQLite. With B’s
conversation indexed and A’s restored from disk, the bare picker showed
only B. Under the same permission error, `cf8ec96860` showed both before
any input. B continued A’s original thread while the directory remained
unreadable. A’s 25,543-byte existing transcript and B’s separate rollout
were preserved. On regular `45078d479d`, all 173 rollout library tests
and the SQLite-backed picker regression passed.
[0.170 successor source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-cf8ec96860) · [Resume guide, including the installed 0.11.0 fallback](../README.md#resuming-local-work-after-switching-accounts)

A separate terminal test found a directory race on `cf8ec96860`. After
B opened the picker, A changed the thread’s directory through
`thread/settings/update` and exited. Both A’s server and SQLite had
the new path. B selected the correct thread and used B’s login, but the
shell ran in the directory recorded when the picker first opened.
An explicit `-C` still selected the requested directory. The failing
terminal record has SHA-256
`5884f265ab21929b7d1ffc77e711da392335f0abfac3184dac625fba51736f13`.

The `11adb87530` picker reads that directory when B selects the thread.
In the same two-account test, B’s actual shell printed the updated
directory and read its marker. An explicit `-C` prevailed, and the
listed directory remained available when the backend could no longer
read the selected metadata. Seven real terminal trials passed, including
the directory controls, restored history beside an unreadable directory,
an A→B→A round trip and the identical-title pagination test with threads
created by each official release. The exact executable passed 18/18
direct comparisons with vanilla 0.170 and 14/18 with vanilla 0.171.
Its named Code Mode host passed the positive and missing-host controls.

The three cwd trials used the byte-identical test on the complete
regular test checkout, which provides its terminal driver. The reconciled
0.170 source includes both the test and its driver. The complete
binary comparison record has SHA-256
`5fc8455060956bfe8b8c84f6c3bc3d0d2d8d2d85846ced706c91c45433512bb7`.
[Directory-corrected source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-11adb87530)

Private main includes the selection-time directory correction and its
terminal regression. All ten protected native picker checks passed on
regular `a384dd5f00`, covering fresh, stale and unavailable metadata,
local eligibility, remote directory behavior and explicit configuration.
The test receipt has SHA-256
`9dbbe2c3a88360bc8d4cd0ecfd6799780a2b685a66d8e8dddb805d28f7c1b2b0`.
[Regular source fix](https://github.com/alabsystems/codex-alab/commit/428d1e46ac26d60ea3ba2d591bd1fd12f6dfabef)

</details>

Each release scenario is a named behavior the fork must demonstrate.
One scenario can require many tests. Results belong to the exact source
revision and upstream review base that produced them.

| Source | What the number counts |
| --- | --- |
| `aa54207c18`, named-resume source reconciled with `0d07274075` | **3 of 78**. Two readers verified build and adoption scenarios 1, 2 and 4. Their full admissions each listed the other 75. |
| `23a3833888`, unified 0.170 review | **64 of 78** on private base `6fe2b3f31d`. Two independent readers accepted the same 64 scenarios and identified exactly 14 awaiting complete evidence. The breakdown below lists all nine feature families. Checkpoint SHA-256 `84e140dfdfa77fa79ee932102fbc76e393d28a0cc60820f2c3f228e9d7b6990a`. |
| `cf8ec96860`, 0.170 review with the readable-history fix | **29 of 78** scenarios verified on this source against private base `5fa8696b89`. Three build-and-adoption scenarios passed 111 distinct Python tests. Seventeen provider-recovery scenarios passed across 466 native tests, including all 105 required by name. Four account-isolation scenarios passed seven Core, five login and one Cloud test. Five exec-handoff scenarios also passed across Core, TUI and app-server. Its CLI separately passed all 18 direct comparisons with vanilla, four provider catalog comparisons and the 48-request wire trial. |
| `d217fa9da3`, working 0.170 picker | **26 of 78** required scenarios verified. All 18 separate vanilla comparisons also passed. |
| `019cbd9f6b`, test-corrected 0.170 release candidate | **63 of 78** required scenarios verified. Of the 15 remaining, 2 have runnable recipes and 13 need complete recipes. The same executable passed all five cross-login trials and all 18 direct vanilla comparisons. |
| `a98666774c`, earlier development comparison | **39 of 97** required scenarios had passed at the earlier checkpoint. That run covered 14 feature families. |

<details>
<summary>Which behaviors have been verified on the unified 0.170 review?</summary>

| ALab feature family | Verified | Required |
| --- | ---: | ---: |
| Account isolation | 4 | 7 |
| Exec handoff | 5 | 11 |
| Provider recovery | 23 | 23 |
| Model discovery | 7 | 8 |
| Native session lifecycle | 8 | 11 |
| Product policy | 2 | 3 |
| Native user interface | 5 | 5 |
| Build and adoption | 4 | 4 |
| Qualification fixtures | 6 | 6 |
| **Total** | **64** | **78** |

Each row counts a scenario only after every required check passes on this source.
Sixty-six scenarios have registered test procedures. Twelve still need complete
procedures. The direct vanilla comparisons and cross-account terminal trials
are reported separately above. [Scenario definitions](../features.toml)

The twelve procedures still to write cover these requirements.

- **Account isolation 2, 6 and 7.** Change account assignment during a refresh, observe the first authenticated request from the new owner, and retire the old account’s pending work.
- **Exec handoff 5 through 10.** Preserve the original goal, queued input and editor state across live handoffs, including cancellation. Activation of the retained-session receiver remains on hold.
- **Native session lifecycle 1, 9 and 11.** Observe a real crash and reclaim, a quota-driven group migration on a qualified package, and queued work and subsequent admission across shutdown and suspension.

The source audit has SHA-256 `c558467ff0b48fb3f8fcfa626b06286b64161d1b15cb4032950d0570a8200979`.

The other two open scenarios already have procedures. Model discovery 6
and product policy 2 both require a successor model to write to a shell
started by its predecessor. In all four trials on this exact review,
macOS denied the nested sandbox operation before the initial shell
created a process handle, including the control with no model change.
The shell probe has SHA-256
`0df10712e869a6226763befdc03f24745be466a8f38b28e3792bfff7df722bff`.
Two independent readers verified all 64 accepted scenarios, the complete
list of 14 outstanding and the original gate record. The final check
has SHA-256 `4e379d7435ae850750f71d5b470b89aafc7fb8afb47509a2fb331ee6f1fd0519`.

</details>

<details>
<summary>Which behaviors make up the 63 of 78 on the 019c release review?</summary>

| ALab feature family | Verified | Required |
| --- | ---: | ---: |
| Account isolation | 4 | 7 |
| Exec handoff | 5 | 11 |
| Provider recovery | 23 | 23 |
| Model discovery | 7 | 8 |
| Native session lifecycle | 7 | 11 |
| Product policy | 2 | 3 |
| Native user interface | 5 | 5 |
| Build and adoption | 4 | 4 |
| Qualification fixtures | 6 | 6 |
| **Total** | **63** | **78** |

The denominator comes from the contracts applicable to this source and
its upstream base. Each scenario counts once when all of its required
checks pass. The 18 direct vanilla comparisons are a separate measure.
[Scenario definitions](../features.toml) · [How earlier counts were calculated](#reading-the-scenario-counts)

</details>

On `cf8ec96860`, the final five verified scenarios exercised exec
handoff cases 1, 2, 3, 4 and 11. Two readers agreed on the exact 49
scenarios still open. The checkpoint has SHA-256
`2ab146e47b61d481f4865fb3b74ee7df152e79a515ab6586786d0342028475e1`.
The run stopped there to prioritize the reproducible directory race.

The reconciled release source `10bd5b2510` combines the directory
correction with the newer regular fork through `a384dd5f00`. Four
conflicts in fork-owned documentation, tests and the banner preview were
resolved. Its upstream branch point and 0.170 tag are unchanged. The
ordinary gate against private base `3b86764769` accepted every required
hook and all nine upstream intake decisions. The seven protected Python
runs passed 111 distinct tests. They qualify three build-and-adoption
scenarios out of the 78 applicable scenarios. Both independent readers
listed the same 75 outstanding. The new feature manifest supplies
complete execution recipes for 66 of the 78 scenarios. The execution
record has SHA-256
`f31e79050f97da4d18198133040f3d8b631d860c28c6abddb4611dbaac3244a2`.
[Reconciled 0.170 source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-10bd5b2510)

The current unified source `23a3833888` also includes the regular fork
through `6fe2b3f31d`. Compared with this newer base, the earlier
reconciliation lacked 22 hooks for reporting review findings. A
conflict-free merge brought them into the leaf. The ordinary gate on
the shared build repository passed its hook and nine-commit intake
checks. Its own seven Python stages qualified three of the 78 scenarios,
with the two readers independently reporting 75 pending. The private
release source still has the exact 0.170 tag and branch point. The new
checkpoint has SHA-256
`f8df285f40fa1f8adb1c79659fac0759ed5714b64cd116db0665fea9969fa9ee`.
[Unified 0.170 source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-23a3833888)

Regular fork commit `cbb9101fad` adds a regression for part of
native-session scenario 11. Suspending an idle thread returned
`NotActive`. That same thread then made exactly one loopback model
request and completed a new turn with the expected answer. The
protected native test passed on its first run. The full release
scenario also requires other refusal, shutdown and persistence paths.
Its status on the frozen 0.170 source remains open. Verification
SHA-256 `80b79d10059e6fffe72337e4ae9d28319fd5485fe3a8f78a161d39263053aff0`.
[Regular regression](https://github.com/alabsystems/codex-alab/commit/cbb9101fadd9fa63adbe46ca71a5376e936fd1ad)

Regular fork commit `fc6cc958a8` exercised all three suspension refusals
in one protected Core test run. A thread accepted a fresh turn after an
idle suspension request. A manual compaction finished after suspension
returned `UnsupportedTask`, and the same thread accepted new work. A third
test closed the fixture’s history writer while the original model request
was in flight. Suspension reported the failed flush. After the writer
reopened, both the original turn and a new turn completed on the same
thread and were present in its saved history. All three selected tests
passed. Native-session scenario 11 still needs the queued-input and
child-persistence checks across its shutdown paths. Verification SHA-256
`3f89e6413353b645015671d8c4db5aaba5945ab2a0d81b96bd6eae14af6c674c`.
[Regular Core tests](https://github.com/alabsystems/codex-alab/commit/fc6cc958a8606306d0f3b92e3e1452f6c354b6a2)

Regular fork `8ba1dadeab` also verified what happens when automatic goal
work is already queued. The tests used the normal recovery worker and
observed Core accept its submission while account assignment was pending.
Reopening that assignment started exactly one model turn on the original
thread. Suspending an active turn instead retired the queued work with no
additional turn in the saved history. Shutting down a local tree retired
the queued work, stopped its root and member and left an independent root
responsive. All four selected native tests passed.

Channel closure remains a separate part of scenario 11. The live submitter
holds a sender while it waits. Dropping that submitter would release the
goal permit before the channel closes. That sequence cannot establish
permit retirement at channel closure. Regular fork `aa16697e53` now
covers child-result persistence at tree suspension. Two children finished
while the parent still held a queued automatic goal. Suspension wrote
each child’s result exactly once to the original parent history and
retired the pending goal. The independent root remained responsive.
All four selected native tests passed. The protected verification has
SHA-256 `a679e39a31da717096064d617da1408e9b8deb82a9b113da92d51f492cea3900`.
The full lifecycle scenario still awaits the channel-close evidence.
The frozen `23a3833888` review remains at 64 of 78.
Native verification SHA-256
`2a5a9654dcd20a24f9aabc7f6631f901a38f75083361b9f54f1871e3043c189a`.
[Queued-goal regressions](https://github.com/alabsystems/codex-alab/commit/8ba1dadeab71b7266f6ff6f233e170be76fa68ce)

The subsequent 23a account and provider runs passed all 13 account
checks, 57 WebSocket checks and 409 transport checks. The two readers
independently accepted 24 scenarios and rejected the same 54 outstanding.
The 24-scenario checkpoint has SHA-256
`b3152ce0ad556cfce1ffbd2b21cbe6968e44a8fd12e01e03985c7635d6726587`.

The native UI run passed all ten selected terminal tests. That met
scenarios 4 and 5. The 20 GiB disk guard stopped the first link required
for scenarios 1 and 3. After space was recovered, the protected build
produced a CLI byte-identical to the named review executable. Both
scenarios passed their complete terminal controls. A separate run also
passed scenario 2. All five native UI scenarios now have release evidence.

In two single-worker runs, terminal rendering interrupted libtest between
printing a test name and printing its outcome. The strict reader recognized
only two of four native UI names and 12 of 14 qualification-fixture
names. It rejected both runs. Fresh protected two-worker runs kept the
original output intact and recorded every required name. All six
qualification-fixture scenarios passed. A durable collector improvement would give
each test executable its own protected libtest completion file and verify
the names alongside the original console output, exit status and suite
counts. The pinned compiler supports `--logfile` and truncates that file at
startup, so multiple test executables need distinct paths.

The weekly quota run found four obsolete test names left after routine
checks became silent. The previous collector ran 21 real tests and then
rejected the run for the missing names. The corrected collector selects
the current tests for an inventory outage, a change of account during
credit selection and quiet preservation of credit. Its four CLI cases
still require the reason and expiry at a 24-hour usage wall and
confirmed restoration at 25 hours. The corrected collector passed 108
Python tests. It then ran the complete scenario on the frozen 0.170
executable with that source’s original test. Preflight and recovery each
sent zero consume requests at 24 hours and exactly one at 25 hours.
Both release readers credited the result. The four-case wire record has
SHA-256 `a244e4693ca22b3de56d06bdef2c22e7f6d4491bd2fd405409c4f9a62f9c1e19`.
[Collector correction](https://github.com/alabsystems/codex-alab/commit/ed56680b6fc124315ffa8b949202dee28dc4dd4f)

**Compared with stable OpenAI 0.171.0**, the same ALab executable passed
14 of the 18 direct checks. Four comparisons found differences. New
`codex exec --remote` and exec-server proxy options change the CLI help.
The new stock CLI also creates status-line skill and marketplace staging
files, and reports a default for `features.mcp_shared_repl_cwd` in
both terminal environments. The app-server comparison matched 26,159
shared schema entries and attributed 992 differences to the sources.
Exec events, app-server initialization, MCP listing and file-based login
passed. The tested macOS executable matched the binary inside OpenAI’s
release archive byte for byte. The exact same test suite passes 18 of 18 against requested 0.170.
[OpenAI 0.171 release notes](https://github.com/openai/codex/releases/tag/rust-v0.171.0)

On the unified review, a three-binary comparison also checked the
OpenAI, Bedrock, Bedrock Runtime and GCP model catalogs. Every response
field matched across the three binaries, with 14, 11, 20 and 6 rows
respectively. Across 27 app-server launches there were 72 unique
loopback inference requests. Each comparison used the same 24 observed
ALab requests plus 24 from its own vanilla binary. Default, enabled and
disabled incremental-tool behavior, model pins, request routing and
history replay passed their controls. Some request fields differed as
expected from ALab defaults. Between the two vanilla versions, the only
additional normalized differences observed were the `exec_command`
login description and the model menu in the `spawn_agent` description.
The three-way comparison has SHA-256
`1eb67471d7ae1ee6f94d6bf5397ce8478742367ea9bd36da4bac0f3833eae752`.
The 0.170 field-by-field result has SHA-256
`c04e65b974e5ea9dbfa6d6e16ae85344b92f818c5cfd8211b260cf85b150caf0`.
These are local protocol observations. The OpenAI catalog case read the
catalog only.

The new release lists six features and six fixes. The 0.170 review
contains its Sol/Astra catalog through OpenAI’s own 0.170 backport.
Five feature bundles arrived later: the default TUI workspace and
previews, non-Vim composer undo/redo, remote exec, terminal device
pairing, and custom MCP status lines. The six specific fixes also
arrived later, including keeping drafts and queued input with their
thread and preserving permission profiles across directory changes.
The separate development candidate `1ea6e85e99` contains all 20
mainline PRs cited for those 12 items. It also contains the mainline
equivalent of the 0.171 status-line backport. The 12-item source
ancestry record has SHA-256
`94c6ece0176a990064e3d548287576f7579bb3b9e0731683ce1ae12ad4baa799`.
[OpenAI changes from 0.170 to 0.171](https://github.com/openai/codex/compare/rust-v0.170.0...rust-v0.171.0) · [ALab development candidate](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-1ea6e85e99)

On official 0.171, B's bare picker showed zero sessions when A had done
work through exec in another project. A broader list query on the same
server returned A's original thread. On ALab, a separate real terminal
trial opened a thread created by official 0.171 under the other login,
at row 26 among identically titled conversations. The continuation used
the selected thread's workspace and the active login, preserved its
25,812 previous transcript bytes and left all 25 neighbors intact.
The comparative binary and terminal record has SHA-256
`847f0eba7349a9e4b024c0a3006b53546c0aedcc30efc1437d99c67bbeabb1a6`.
[Vanilla account filter at 0.171](https://github.com/openai/codex/blob/ccf9be8cc458bd6b8336fe84d68d8893f2691a5e/codex-rs/tui/src/resume_picker.rs#L2619)

An additional comparison exercised the 0.170 WebSocket fallback during
a resumed turn. Both binaries retried once over HTTP after an oversized
request was rejected before any response event. Both retried over
WebSocket for an ordinary close or for the same rejection after a response
had started. All six trials preserved the full ordered conversation input.
[WebSocket comparison and controls](websocket-close-vanilla-2026-11-19.md)

A source review of the first 20 of those 49 development commits found
that vanilla still limits the picker to the current account. All seven checked CLI,
picker and thread-list files were identical across that interval.
OpenAI added per-model Daybreak availability to `model/list`, reused the
parsed fallback model catalog, changed how tool descriptions fit its prompt
budget, and boxed the existing credential-loading future. Those changes
landed after the 0.170.0 branch point and are outside the stable target.
The 20-commit source review has SHA-256
`fb05ebf79596eda3909ef8cf60cafb555a5cbc71df08e41fa720ae681b713ff2`.
[OpenAI commits compared](https://github.com/openai/codex/compare/b0ecad9cf9c08318d79e9d7f91520e609d1c4d1a...99870c4f4921ee03f820e8e322ef97ed671ea6ef) · [Vanilla picker at the comparison tip](https://github.com/openai/codex/blob/99870c4f4921ee03f820e8e322ef97ed671ea6ef/codex-rs/tui/src/resume_picker.rs#L2633)

The next two upstream commits keep Guardian scoring active for serving
probes and change the cloud icon shown on cloud threads. The latter
updates two picker snapshots. At that revision the vanilla picker still sends
`current_account_only: true` when listing local conversations.
[Two upstream changes](https://github.com/openai/codex/compare/99870c4f4921ee03f820e8e322ef97ed671ea6ef...6eea32808167c4b87508e9611ba116f061ee315c) · [Vanilla picker at that revision](https://github.com/openai/codex/blob/6eea32808167c4b87508e9611ba116f061ee315c/codex-rs/tui/src/resume_picker.rs#L2633)

The following six changes render LaTeX underbraces, refresh open sidebar
previews, tolerate unrelated mount warnings during macOS installer
cleanup, centralize config-diagnostic schema selection, enforce refreshed
Guardian policy on running threads and let hosts select Guardian’s
inference provider. The complete picker source at `5f8c36e836` is
byte-for-byte identical to `6eea328081`. Vanilla still requests the
current account’s local conversations by default.
[Six newer OpenAI changes](https://github.com/openai/codex/compare/6eea32808167c4b87508e9611ba116f061ee315c...5f8c36e8368cb97720bd9c21b2dafacccbdc9679) · [Vanilla picker at that revision](https://github.com/openai/codex/blob/5f8c36e8368cb97720bd9c21b2dafacccbdc9679/codex-rs/tui/src/resume_picker.rs#L2633)

The following two changes store MCP model-search state in new conversation
checkpoints, retain a fallback for old records, and explain a Windows
sandbox logon-policy error. The current vanilla picker still requests only
the active account’s local conversations.
[MCP and Windows changes](https://github.com/openai/codex/compare/5f8c36e8368cb97720bd9c21b2dafacccbdc9679...bc75380d6a5ee05584ad432162157799a8a2d646) · [Vanilla picker at that revision](https://github.com/openai/codex/blob/bc75380d6a5ee05584ad432162157799a8a2d646/codex-rs/tui/src/resume_picker.rs#L2633)

In the next nine commits, the app server began returning from
`thread/resume` before replaying pending server requests. It waits for
one outstanding write per connection and stops replay if the client
disconnects or unsubscribes. Guardian also refreshes approval policy
when the current owner logs in, on logout and on config reload.
The same vanilla picker still restricts local history to the current
account. These changes are on OpenAI main after the stable branch point.
[Pending-request replay](https://github.com/openai/codex/commit/6633c6e09ab64bb1cb15c16ce8ad1da927406c06) · [Guardian policy refresh](https://github.com/openai/codex/commit/db263d4f5471f4fd2e91ae0955556ace66aeb1c5) · [Nine upstream changes](https://github.com/openai/codex/compare/bc75380d6a5ee05584ad432162157799a8a2d646...5bdb5e8b3c4b43e4a10e53f2f8a3813d318d1f9c) · [Latest checked vanilla picker](https://github.com/openai/codex/blob/5bdb5e8b3c4b43e4a10e53f2f8a3813d318d1f9c/codex-rs/tui/src/resume_picker.rs#L2633)

Regular private main now carries a confirmed Accounts-pane subscription
selection through restart.
On a compatible aterm host, confirming a destination rechecks its roster
revision, slot and verified subscription reference before switching. This
lets two enrolled subscriptions with the same masked email remain distinct.
The bare resume picker continues local history under the account already
signed in. The subscription-switch change landed after the `019cbd9f6b`
review source.
[Account selection and restart behavior](../design/accounts-assignment.md)

The next OpenAI commit changed rollout budget accounting. Exhausting a
host-wide budget stops new work, while exhausting a turn’s budget returns
a session-budget error. The vanilla conversation picker at this revision
still requests only the signed-in account’s threads.
[Budget change](https://github.com/openai/codex/commit/26f70f94609c213b6eb7fc048ac6c2a88f7c9f59) · [Vanilla picker](https://github.com/openai/codex/blob/26f70f94609c213b6eb7fc048ac6c2a88f7c9f59/codex-rs/tui/src/resume_picker.rs#L2633)

Two further OpenAI changes shared slash-command argument range handling
between editing and dispatch, and let Guardian retry a failed model
catalog fetch in the background. The latter lets a first tool call
complete while discovery is pending and allows a later call to be
scored once the catalog is ready. The local conversation list still
sets `current_account_only` to true.
[Two upstream changes](https://github.com/openai/codex/compare/26f70f94609c213b6eb7fc048ac6c2a88f7c9f59...ead489ac4d06b13a69909173469a17be126348a4) · [Current vanilla picker](https://github.com/openai/codex/blob/ead489ac4d06b13a69909173469a17be126348a4/codex-rs/tui/src/resume_picker.rs#L2633)

OpenAI main subsequently centralized context ID allocation and rollout
reconstruction. The preceding and current versions both reuse a valid
saved compaction window. The stored thread UUID and active-account
picker filter are unchanged by this commit. A Git merge rehearsal from
ALab's 427-commit development candidate to `583ec348cf` reported 21
conflicting paths. Four fall under committed removal rules, two are
generated protocol exports, one is the workspace lockfile, and 14
are source or test files. The result has SHA-256
`ea4490d8c1407f9a44ed7690873d5a91b3622841efaa84486ebe5360ea2b0b27`.
[Context ID change](https://github.com/openai/codex/commit/583ec348cf) · [Vanilla picker on main](https://github.com/openai/codex/blob/583ec348cf/codex-rs/tui/src/resume_picker.rs#L2633)

The next six upstream main commits include faster preview scanning of
rollout files and restoration of persisted MCP tool results by item ID
when reading full thread history. The scanner keeps its existing limits
and filters. The exact stock picker still asks for the signed-in
account’s local conversations. The new cumulative merge rehearsal reports
22 conflicting paths, including a new conflict in the app-server thread
processor. Its receipt has SHA-256
`25d5e9ff93eeac54511b80370b4956ff520733f9c8ae73484902b936561b3b32`.
[Rollout preview scanning](https://github.com/openai/codex/pull/62636) · [Persisted MCP results](https://github.com/openai/codex/pull/62641) · [Current vanilla picker](https://github.com/openai/codex/blob/7642e80723386d113ee5c95647164f72e3d68d6d/codex-rs/tui/src/resume_picker.rs#L2633)

[Detailed release steps](#remaining-release-work)

### Resuming work after changing logins

The corrected `cf8ec96860` executable also continued an official
0.170 conversation at row 26 after a same-title search. The new account
sent the continuation, used the original project, and preserved the
26,079-byte history prefix and all 25 neighboring rollouts. Its
cross-account, permission and vanilla comparison record has SHA-256
`0f80e6f62e7da6a20ff2837ce65751e24da367aadf8f751244029889f2ee56ce`.
The matched CLI and Code Mode host record has SHA-256
`edf3f501ead57d1a2d8593a5c0d44fc8862aa4d9247ca8cf862b31beec9d0eb8`.

The 0.170 review CLI also passed a two-page selection test. A owned all
25 conversations on the first page. A’s last conversation and B’s older
conversation on page two had the same title. A used bare `codex resume`,
searched that title, expanded both transcripts and chose B’s. The next
request used A’s credentials and a real shell ran in B’s saved project.
B’s prior history and all 25 sibling rollouts remained byte-identical.
The same selection also passed while signed in as B. The independent replay
has SHA-256 `983b4530a028b466f358190807b6e76935bfde2ace29eef633d65275aeed5abe`.
The complete `d217fa9da3` review package also passed the same selection
from a signed 0.11.0 conversation. It preserved the older conversation’s
25,790-byte history prefix and the original bytes of all 25 neighbors.
Its independent result has SHA-256
`6b219ec1598a74a56da780358f89ec1f2398b2b089c435147e2fb8f6b32975d4`.
On `019cbd9f6b`, official OpenAI Codex 0.170.0 also created B’s
conversation using its standard `auth.json`. A, signed in through a
separate credential file, found B’s conversation at row 26 with bare
`codex resume`. The picker showed both same-title transcripts. Selecting
B’s row preserved its 26,039-byte history prefix and all 25 neighbors.
The next turn used A’s credentials and ran a shell in B’s original project.
An independent run selected B’s row using B’s original login. The three-run
replay, including a two-named-login control, has SHA-256
`bcb2e477675013d6138d6587db749cfaf9d49e10dc9bf80be2ebc576417096ed`.
[Paged duplicate-title smoke](../smoke/account_resume_paged_titles.py)

On the tagged review build `d217fa9da3`, sign in as B and run
**`codex resume`**. The picker shows
active conversations across all local accounts and projects in the same Codex
home, newest first. Select A's row. If prompted for a working directory,
choose **Use session directory** if the project is still there. If you have
moved the project, launch the picker in its new directory. When the saved
path is gone, **Use current directory** is highlighted. Press **Enter**. The thread keeps A's history and tool
results, and B authenticates the next turn. On signed ALab 0.11.0 the
working command is **`codex --no-daemon resume <thread-id>`**.
In the same cross-project, first-login trial, vanilla 0.170.0 opened its
bare picker with “No sessions yet.” Its app server could still retrieve
A’s thread. ALab’s review build showed it immediately.
[User guide](../README.md#resuming-local-work-after-switching-accounts)

Two trials on the final `d217fa9da3` review installation exercised the login
switch itself. With no conversations of its own and an expired credential,
B opened the bare picker and saw A’s sole signed-package conversation.
B renewed its credential once and resumed that thread with its tool history.
A’s credential file stayed byte-for-byte identical. In the other trial,
the local daemon kept serving A while the picker used a separately selected
B login with the same email and a different workspace. B continued A’s
vanilla-created thread. Both model requests used B’s workspace, and both
credential files kept their original bytes. All four terminal processes
exited normally. The verification record has SHA-256
`710dd5d653022d126396bb2d944694e56aa899a322c750cae9e97240ad5ebf25`.

A separate trial kept the original conversation loaded under A. B saw it
in the bare picker and could read the full thread while A still owned it.
On selection, the terminal displayed “This conversation is open in another
app” and “Close it there and press R to continue here.” The app-server
refused a second writer before any model request or change to the original
history file. Once A closed, B’s next bare resume continued the original
thread. The review-build and corrected-source records have SHA-256
`826cd90ef59a809dc16f1c808c97a1beb9f100d2a511067c130b564a6574678d`
and `ba17e40054a513e74f055c08dafaf6c003e4f5da71fa5cf421c81112fd352b15`.

With a live official 0.170 app-server as A’s original owner, B also
stayed in the **same terminal** across the handoff. A closed the
original session. B pressed lowercase **r**, then sent its next turn.
Both new inference requests used B’s login and the original thread ID.
The shell ran in A’s project, A’s original command ran exactly once,
and the first 25,251 bytes of history stayed intact. The terminal
retry record has SHA-256
`14d8e58891bcb86451045425bd627fe4d1ad7ba31e7afbb592020c09294b40aa`.

The repeatable writer retry test passed with the official 0.170
app-server as the original writer and
both tagged review executables as the destination. Each trial retained
the original 25,742-byte history prefix and observed one destination
terminal acknowledge every key through retry and completion. The
same-build control also passed. Four negative controls rejected a wrong
credential, a substituted thread ID, missing history and altered tool results.
The combined record has SHA-256
`af6f1649967ee42ec085eb2f58f1ecc126a7939d5aa71a882738e322e89680c5`.
The ordinary cross-login and post-quota resume tests also passed after
integration. Their verification record has SHA-256
`4e45bf893c4e1e30b7052e4c00f531e900dae261a5d94e7c19333bbe5c4830e3`.
[Repeatable writer retry test](../smoke/account_resume_writer.py)

An early **r** while A still owned the thread repainted the read-only
transcript. Typing a message and pressing Enter left the saved history
intact and sent zero model requests. After A closed the session, a
second **r** let B continue in the same terminal. Both tagged review
builds passed. A control that ignored the first **r** failed. The writer
regression now checks every user and assistant message on both B requests.
Adding an unrelated sentence to either request also failed. The early-retry
and message verification records have SHA-256
`52ffe46a3b66f786b0cbe686ccc81bbe990d15d1f1da479c7cba6564597c5ba0`
and `095e7a0757d3844af1ab89694b1ec1bd6ee6171053cd4b1abe76b1034f0405b4`.

Four login round trips exercised both tagged review builds. A began a
conversation in official 0.170.0 with a shared login file or in signed
ALab 0.11.0 with separate named logins. B used bare `codex resume` and
added a turn. A then ran bare `codex resume` from a third directory,
saw both conversations, selected the original, and added a third turn
using A’s login and workspace. Both earlier turns and tool results
survived. The other conversation remained byte-for-byte identical.
The independent audit matched all 24 model requests to the exact saved
messages, tool records, thread, turn and active fixture login. All 12
picker terminals exited normally, and all 36 deliberately altered controls
failed. These tests used synthetic accounts and a local provider. The
round-trip record has SHA-256
`ae29feed15dedc83cac0b930d622a0bd5d20f2244596a4b32f45247a5edd4377`.
[Round-trip regression](../smoke/account_resume_return.py)

Starting A in the official interactive terminal also exercised its
background server. A exited normally. B could read the original thread
immediately, but the official server still held it. In a separate trial
the thread disappeared from the server's loaded list 59.9 seconds after
A exited. B then opened the bare picker and continued, followed by A.
The original conversation retained its first 50,111 bytes. Starting
in signed ALab's terminal also passed. The terminal-origin record has
SHA-256 `5ec4d0fb7c2a5461301f2f77e4384f6ec093ee4a74fc8fbc87fe94b4cc14c914`.
[Official 0.170 default idle delay](https://github.com/openai/codex/blob/29eb79e9866b3262a777fdf8195c1b9e63684c91/codex-rs/core/src/config/mod.rs#L4148)

On both tagged review executables, B kept the same terminal open from
the first read-only view. A’s daemon released the thread after 60.35
and 60.45 seconds from the first observation after exit. B pressed
**r** and continued under B’s login. A then reopened the conversation
with B’s changes. While the thread was locked, B sent zero inference
requests. The two trials preserved 49,739 and 50,420 prior history bytes.
The test also checked that the fixture's daemon exited after a deliberately
interrupted seed. The two verification records have SHA-256
`d772026445b6a5e056d386cd61867d453d83ea6f65a0bd28191832296eb32bd7`
and `18452bb003bfd2df5e8e8cd8954c5591aeaa90d072153951ec5fdc6bc89ee69b`.
[Official terminal handoff regression](../smoke/account_resume_daemon.py)

A separate test exercised the command for the currently signed package.
A started the conversation with official OpenAI 0.170.0. B used the
explicit thread ID with signed ALab 0.11.0 and continued the same thread,
including its tool history, with B's own credentials. The signed fallback
record has SHA-256
`55ff1d5cb03768a5c4aefda8010e7ff3184c74f4dbe50a69c3e3528199f6745e`.

If the ID is missing on signed 0.11.0, the built-in dry-run command
`codex --no-daemon migrate-rollouts --verbose` prints IDs. The signed
app server can also list indexed sessions by preview and project. In
a shared home with 26 conversations, B found A on the second page and
continued its original thread. Both lookups preserved the 26 rollouts,
and listing made zero model requests. The verification record has SHA-256
`9f84b89d7e2c08d83f0d040951eaca3a808ce3832111d6653a3453cf97e1df0a`.

Two additional archive trials used tagged executable `4893d713e7`.
A created an exec conversation using signed ALab 0.11.0 or official
OpenAI 0.170.0, then archived it. From another project, B saw its own
conversation in **Active** and A’s in **Archived**. Selecting A’s row
restored the original thread and appended B’s new turn in A’s directory.
The original 25,540 and 25,556 bytes of archived history were preserved.
All four bare-picker processes exited normally. The archive record has
SHA-256 `e41d54ad90bcafe8dcf1c477558d4ad74235c48702cff2cf11f72a086857056a`.

In another trial, both accounts saved a thread called “Release notes” in
the same directory. B saw two rows. **Ctrl-E** on each row showed its
distinct transcript and full ID. B resumed each original separately,
and only the selected conversation changed. Filtering to **Current**
showed B's original thread alone. All three bare-picker processes exited
normally. The verification record has SHA-256
`5a9695d9ea1fff50f5b38285a430a35b9766fe89fa109b1a9073b7e6aa518654`.

Two six-case runs repeated the moved-project and same-title scenarios from
signed ALab and official OpenAI exec histories, plus an ordinary picker and
archive control. All 12 original rollout prefixes survived. B authenticated
every continuation, and all 32 bare-picker processes exited normally.
Cancelling the moved-project prompt preserved both complete SQLite rows
and both histories. Choosing the new directory then ran B’s command there.
The matrix has SHA-256 `b4832fd2d3b0df85476703651d0bb180659ea8779755dbeea3c19a632202ad8b`.

On the reviewed executable `4893d713e7`, pressing Enter at the directory
prompt after a move kept the vanished path. The next shell call failed
before spawning, with OS error 2. Choosing option 2 worked. The three
controls have SHA-256
`e55ac7aea16a997eeb0afe5af0c22624cc38e1f1a3fe629e1f4e2b02ce180580`.

The corrected regular CLI at `5002e17fe1` then passed two moved-project
trials using **Enter** as the first key at the directory prompt. Both
restored A’s original ID under B and ran the shell command in the new
physical location. The signed and vanilla histories kept their 25,896
and 25,912 prior bytes. Three controls exercised the ordinary, same-title
and archive pickers. All 13 interactive processes exited normally. The
five-case record has SHA-256
`903f3ca7a526e31f87853b6e496ebff8682585b972c5b84b818d8fc6b3bd3071`.

The exact 0.170 review executable at `d217fa9da3` repeated all five cases
and the four wire checks. Sixteen native UI tests also passed, including
all four directory-default regressions and the integrated queue and
Autopilot controls. The selected test roster has SHA-256
`9bc14dda1b1201a4f0ea57afdb0890e6e849b052f02516004e947f2ac96c20ab`. B’s first Enter reopened the signed and official
histories in the new directory and preserved all 25,897 and 25,910 prior
bytes. All 13 picker processes exited normally. A separate run used the
standalone review executable directly with signed named-account
credentials and preserved all 26,050 prior bytes. Its binary SHA-256 is
`64458cfdfd525dd5106245e86b57af6a8ef5165195afd74aa9805e6cef0149d8`.
The final local review layout includes the matching Code Mode host. Its
protected stdio trial verified persistent cells, delegated tools and
termination using the colocated helper. A fresh bare-picker trial reopened
a signed 0.11.0 thread and sent both new inference requests as B. The
assembled-layout receipt has SHA-256
`c7bafd9453818c764d901bc0a5cbd4793816f37d49d89e031ac10ee2dfc51076`.
The five-case receipt has SHA-256
`50a027d8d590a022091583cd3366a74e39603bf0b3078587bcf469d8b86dbc98`.
The same executable passed all 18 comparisons with official OpenAI 0.170.0,
including the 77 help pages. The app-server schema comparison found 26,155
shared definitions, properties and enum values and accounted for 174
differences. The comparison record has SHA-256
`cdb441f03aec2b988ad0c5a6824526f34db002ad5455aacc6f18c1edb85e0dd6`.
The four model catalogs and all 48 provider requests also matched on this
source. In two histories of 10,001 conversations, B found A’s oldest in
2.94 and 3.02 seconds. All 43 fields of the other 10,000 index rows were
preserved, and browsing made zero model requests. The combined record has
SHA-256 `855c02d0d324ac3b1e50674a45ff382c0656710038361ccb164f82e6ef5405bd`.

Seven terminal trials passed on the earlier tagged binary. They covered B's first
login, a thread on page two, A's daemon still running, a missing or stale
index entry, B renewing an expired credential, A reverting a turn, and a
conversation created by the released OpenAI 0.170.0 executable. The
observer checked the thread ID, surviving history and request credentials.
In the vanilla-created conversation, B ran the next tool in A's original
directory. A separate trial recovered compressed history. In two homes with
10,001 sessions each, B found A's oldest thread in 2.53 and 2.70 seconds.
All rollout files survived and the picker made zero model requests.

Two live pickers using separate account credentials also opened the same
home while the original thread's row was missing, then again with its
preview blank. Both accounts saw both threads, and filtering to the current
account showed the corresponding thread. Viewing sent zero model requests.
Both credential files, both rollout files and every field of the other
thread stayed identical. The blank row recovered exactly. Reconstructing
the deleted row from its rollout reproduced 40 of 43 fields exactly.
The three time fields differed from the lost database values by 3–399 ms.
The concurrent-session record has SHA-256
`db2a52deb18e9d04ab34a379bd9531e9621932eb96c29c56e7073341c6f7cce7`.

### Tagged source verification

The corrected `cf8ec96860` has its own 78-scenario request. The native
gate observed private base `5fa8696b89`; that base and the predecessor’s
`91c8341de0` include the same upstream frontier and request the same 78
scenario definitions. Results below are credited to the exact source
and recorded base. On the current base, two independent readers
recognized the three build-and-adoption rows and 17 provider-recovery
rows, and rejected the remaining 58. The 20-row receipt has SHA-256
`2d208d1059c87b513708e0e541fd224a0cf5e475551d13ca5e6fc3d63114e29d`.

The next checkpoint verified account-isolation scenarios 1, 3, 4 and 5.
The tests changed accounts across HTTP requests and WebSocket reuse,
and checked that retired credentials and clients cannot send a new
request. All 13 named tests passed. Both readers accepted 24 of 78 and
identified the same 54 remaining. The account checkpoint has SHA-256
`d7fa26328f6f49f850d7b29e158a279ce091238505bde3ca9c9a694362819635`.

Review source `e5c6a5fe13` also carries the fork's reconnect keyboard
shortcut and diagnostic database retention changes. The local resume picker,
login scope, rollout, login and thread-list protocol sources match the
terminal-tested `4893d713e7` byte for byte. The ten-object comparison has
SHA-256 `c98125da6634f5d7fe9d476584c32391ccc11ffa1ec25e525b5ebffbaf1dc2c4`.
The previous revision stopped at the missing-hook check. With the actual implementations and
tests carried over, the ordinary gate accepted all nine upstream intake
decisions against fork main `00d64a9152`, then requested fresh evidence
for 78 scenarios. Its initial gate record has SHA-256
`41fb2c6e0e81422610c5c980a76e7d4bd2246045733ec99014fa390722fa9538`.
[Earlier tagged review source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-e5c6a5fe13)

A separate user-facing review source, `d217fa9da3`, carries the missing
local project directory correction plus ALab’s newer queue, Autopilot and
banner changes. Its ordinary gate accepted all nine upstream changes and
the complete hook counts against private main `31d0010a6c`. Comparing the
five additional fork patches found identical changed lines in every patch.
The feature-tool suite ran 212 tests with one skip. On this exact source,
two independent readers verified the first three of 78 scenarios. Seven
protected stages covered all 111 distinct tests for build-and-adoption 1,
2 and 4. Both readers refused the remaining 75. The portable record has
SHA-256 `23633630bac9b59386d97e4e8ff94fbb0167ce06dbc5a71e4943c17218f261cd`. The gate record has SHA-256
`32e78a94adbeb46542e0fb8de946f6e711f2c1cfc982d62d13115dc9aad04ac8`.
The next six exact-source collections brought this build to **26 of 78**
verified scenarios. They cover five account/provider collection groups
and native UI scenarios 2, 4 and 5. Both independent readers identified
the same 52 outstanding scenarios. The checkpoint has SHA-256
`90d3926c5ea816e0e3d2d712db7c01cd47132eda8b3bb2c07e7200eb3ce1a6ad`.
[Tagged picker source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-d217fa9da3)

The next release candidate `019cbd9f6b` changes only two test files. Its
independent native check accepted the nine upstream intake decisions and
all hooks against private main `91c8341de0`. The corrected original-tree
suspension scenario passed in 12.73 seconds. Both readers accepted that
one exact-source pass and refused the other 77. Its portable record has
SHA-256 `4d28ffe094ec965d7141768de1c117867fecc2dc9cd053d079ffa67245e513c4`.
A separate exact-source collection verified three build/adoption scenarios
across 111 named Python tests. The combined four-scenario record has
SHA-256 `be7438cca8fa6cfa9b13cad67fe3be8e76b988a0dc852e67735910782dcd3db7`.
The next native UI run verified draft isolation across side chats and
pasted-image persistence across resume, taking the total to six.
Both readers accepted those six and listed the other 72. The checkpoint
receipt has SHA-256
`9978a331694c4dd5c671daaceba69704e29cf6542737d929828a9f86743e3ce1`.

The source then reached **30 of 78**. All five native UI scenarios,
four account-isolation scenarios (1, 3, 4 and 5) and 17 provider-recovery
scenarios (3 through 19) passed alongside the original suspended-input
and three build/adoption scenarios. Both readers listed 48 remaining.
The combined checkpoint receipt has SHA-256
`55ff9ffea2a96610f4e92cce7af9186eebd999c793070adc06c7e7d074a3e7dc`.

Four native-session scenarios (2 through 5) then passed, bringing
the count to **34 of 78**. Both readers agreed. The next protected
run for scenarios 7 and 8 stopped when the volume reached 19.96 GiB
free against its 20 GiB working minimum. A separate already-built TUI
target also required dependency revalidation. It stopped at 19.98 GiB.
Neither interrupted run counted. The 34-scenario checkpoint receipt has
SHA-256
`2140a2c0381ece78a47f41bb88c2062d51592c2b1ef6f46ecd0d25fea7b8318e`.

A separate Core attempt stopped before testing when the release-download
host returned HTTP 403 for the pinned Trust 15698 manifest. A later
retry authenticated that compiler and reached the disk guard at 19.98 GiB.
The eight Core tests for exec handoff scenario 2 never ran. Its stage
record has SHA-256
`de7fa084b7a5fbc86656b89fddbedbdbcabbfd1c2143a65b5a22b18f55f91976`.
A separate regular-main regression for native-session scenario 10 reached
compilation and stopped at 19.99 GiB, before its first test. Its patch,
four proposed test selectors and replay instructions are saved for the
next build with enough space. The diagnostic archive has SHA-256
`a8cd74a4811f90e9536fe93abce325dd3e1431cf0a1d0d02a72b3401da6c012d`.
At that checkpoint the inventory listed **44 outstanding scenarios**. Of these, **31** have
runnable recipes and **13** still need a complete recipe. Both native
test processes and the compiler selection process exited. The borrowed
checkout was restored clean at its original revision. The final
source-specific inventory has SHA-256
`22b54b5553e0fc8cc4b70a89f57d57d2429ab482a0e34e137de9c0cbe66a2b62`.

With 27.47 GiB free, the exact candidate resumed qualification. Each
checkpoint below combines evidence for a single source and review base.
Two independent readers checked the complete set and identified the same
outstanding scenarios.

| Verified | Newly demonstrated behavior | Checkpoint SHA-256 |
| --- | --- | --- |
| 35/78 | Eight Core tests passed for exec handoff scenario 2. | `c3bb0708dea8257e82288eadfdfd78e86819d1cdeeba5972136adcd6c75c096c` |
| 38/78 | Seven API fixture tests covered incomplete HTTP bodies, cleanup and retaining evidence from failing requests. | `0bb51362052ebd58397468da781753656121d8c823806b75e9717b1cb9cd6b41` |
| 41/78 | Eight TUI tests covered returning the original editor and its queue, selecting the current account’s model defaults, and retrying without repeating an acknowledged tool. | `87f094f903d94a909e17bb70c2a695cc4621093a8cb5f282859e320c9311801b` |
| 44/78 | Native-session scenarios 6 through 8 covered interrupted work and preserving the original goal when tool-call identifiers collide. | `359976bfd182aaa363b92fea5f687e71c0089b058e523f4ebd1198a75ab51018` |
| 46/78 | Fourteen TUI fixture tests completed two scenarios covering terminal diagnostics and custody of saved state. | `c12131c9dc1eccfbe4a365fc3c02d376c3d5c0e2276a27aeababec800f0b0acb` |
| 48/78 | Exec-handoff scenarios 3 and 4 each passed separately on the same revision. | `7bafbfba7d2aa0528466ca8986c8a1ff97a40773c19256c8fab8f214a99b0fc7` |
| 50/78 | Five tests across exec and app-server verified catalog failures and shutdown of workers with pending model checks. | `6d3051de25c74d88d8e49f51ecc736f649ffba18bd67be95469879b62915f0f3` |
| 53/78 | The CLI passed catalog-readiness and saved-effort wire checks. The build tests verified who owns an update across regular and development versions. | `247e79bb33dcb599b380654c4daf5affd9756fe7ec6e741218b9f00f2e5e8a39` |
| 56/78 | Product-policy scenarios 1 and 3 and model-discovery scenario 4 passed across their native and CLI checks. | `b33ad1315a56bf3c80b70d0d6b73c73d7d489b777008482603855c1b11fa091a` |

The first combined run of exec-handoff scenarios 3 and 4 exposed an
intermittent failure. All six Core tests passed. Five of six app-server
tests passed. In the sixth, recovery rejected a persisted child with no
registered identity before reaching the expected account check. Both
scenarios passed when run separately, including the same failing test.
The original combined log has SHA-256
`d0aeec1319f1e7cdb0f3a541970e19869a43d0704816269c8a545343c130c3c4`.

On the same test binary, the parallel six-test group failed three times
with a soft limit of 256 open files. Two runs reported file descriptor
exhaustion and one reproduced the missing-identity error exactly. The
serial group and the single test passed with that limit. Four parallel
runs passed with the gate’s normal open-file allowance. The regular
qualification runner now applies that allowance to native workers within
the inherited hard limit. The original failure log omitted the underlying
OS error. The diagnostic record has SHA-256
`ef51b3c89e076bdebb9892f4089defdb84bda667d654b4e5ab8f33aa6bb274be`.

Model-discovery scenario 7 passed the native and CLI checks for fleet
model policy. The 57-scenario checkpoint has SHA-256
`71b21e5c07bc36212bd1cec32df21c8cdca8a0d2977b3b0dce6335e58f93975f`.

Two scenarios, model-discovery 6 and product-policy 2, require a successor
model to write to a shell started by its predecessor. In all four paths,
including the control without model retirement, the protected macOS
runner refused the original nested shell sandbox with exit 71. No shell
PID marker was created. Both scenarios remain open on this source.
In a separate run under its ordinary workspace-write sandbox, the same
binary passed all four paths and wrote to the original PID and session.
The regular test now records the first shell’s error and PID-marker
presence before checking the number of model requests.
The independent diagnostic has SHA-256
`093531e1f57740f0b41cb8d14fb6ebec6c97590aefc3379b101834a804ff5b32`.

The original standalone recipe for qualification-fixtures scenario 1
also selected an unrelated test crate with zero matching tests. The
strict reader rejected that run despite the two intended terminal tests
passing. The corrected regular recipe selects the crate that owns those
tests. Both passed on the frozen binary under the protected runner.
The corrected-recipe record has SHA-256
`e04e7fe6a9c8fc25809ce9703e25d430a20fcdfd9fa07814d21d0a7a28f85369`.

On the unchanged release source and original recipe, running scenarios
1, 4 and 5 together exercised both selected crates. All 27 required names
passed across seven stages. Both readers accepted **58 of 78** and counted
the new scenario once. Its checkpoint has SHA-256
`0cbb939b567fa2150c253ea2eaa63a87a7b67e3bcbee925bedca0d7f02da1128`.

The standalone provider-recovery 20/22 run passed all 25 quota checks
but selected two additional crates with no matching tests. The regular
collector now assigns each registered provider check to its owning crate.
Under the protected runner, 17 Autopilot, 25 quota and four Kimi checks
all passed on the frozen test executables. The old partial result still
fails validation. The separate verification record has SHA-256
`f6bdca2fbc4a515cdc6af7c73a84603bc60c704c7f3da20f593b55d7efc77853`.

The final five-scenario provider run completed five stages. Quota recovery
and core supplied every check required by provider scenario 20. The
disk guard stopped compilation of the Autopilot integration tests at
18.63 GiB free. Two independent readers admitted only scenario 20 from
that run and reproduced **59 of 78** on the exact tagged source.
At that checkpoint four provider scenarios (1, 2, 22 and 23) needed native runs with
enough disk. Model-discovery 6 and product-policy 2 need a protected
runner that permits the tested shell to start. Thirteen other scenarios
still need complete recipes. The checkpoint has SHA-256
`f2f978cb85338888689a1f3d82a9873e241e617975c440ba88922ec9aefe0c76`.
The source-specific inventory has SHA-256
`8e9ae262ecac79ad146aa01e80a2a32ff6dc49cff067e8326a7e5e77663ca676`.

The corrected collector then ran provider scenario 22 from scratch on
the frozen release source. All seven original groups and all five stages
passed. The rebuilt CLI matched the frozen executable byte for byte.
During both preflight and response recovery it made zero credit redemptions
at 24 hours before reset and one at 25 hours. Two independent readers
verified **60 of 78** and refused full admission with the same 18 outstanding.
Three provider scenarios (1, 2 and 23) need the native integration build.
Two shell scenarios need a compatible protected environment, and 13
scenarios still have no complete recipe on this release source.
The checkpoint has SHA-256
`5f0556d6192d348be364ef490d4119fcf49a95ee9765f8dc5ad43dfb4d24f0e3`.

Provider scenario 23 then passed on the same source. The earlier disk
stop occurred while four integration binaries were linking and stripping
at the same time. With one compilation job, all four binaries built,
and all 27 tests in their suites passed. The tagged CLI retried a transient
Kimi rejection and treated each of the four quota limits as terminal.
All 14 named checks passed. Both readers accepted **61 of 78** and refused
full admission for the same 17 outstanding scenarios. The checkpoint
has SHA-256 `33ff3a9fdeec6a7cf93748bd25ea4d926dd38135b7bb02cdfe9a439b74d5afad`.

The same clean checkout went on to pass all 37 required checks for
provider scenario 2. These cover resuming the original task after HTTP or
WebSocket handshake errors, without duplicating input or completed tools,
and respecting retry deadlines and exhaustion. Both readers registered
**62 of 78**. The remaining registered scenarios are provider recovery 1,
model discovery 6 and product policy 2. The other 13 lack complete recipes
on this release source. The checkpoint has SHA-256
`7987380b5c16a7e7b3df0feba9c9ecd693d2b76feb13d159636ccfc5f72f2d0e`.

A fresh one-job attempt at provider scenario 1 started with 24.57 GiB
free. Four of its five stages passed, including all 22 named checks
outside the queue suite. The cold Autopilot integration binary built
and its turn check passed. While compiling the app-server test support
for the remaining three queue checks, the disk guard stopped the run at
19.85 GiB free. No queue test ran on that attempt, so the release count
stayed at **62 of 78**. The stopped queue log has SHA-256
`bbd0f6ba5433398c4786971262466fbf5a8af1ff1d4a56095b810712e146f01a`.
Both independent readers reproduced that 62-scenario checkpoint. Its
per-feature inventory has SHA-256
`50d42aa4514c72bce9942306039a90ea624f2437f86ec86f898ae5e03c0fe81a`.

A subsequent one-job run started with 26.25 GiB free. All five stages
and all 25 required checks for provider recovery scenario 1 passed.
The final three checks exercised queue relay, turn-specific mail, and
a queued item held after a typed refusal. This completes **23 of 23
provider scenarios** on `019cbd9f6b`. Both readers accepted **63 of
78** overall and identified the same 15 remaining. Model discovery 6
and product policy 2 still require protected runs. The remaining
13 scenarios lack complete recipes on this source. The 63-scenario
receipt has SHA-256
`c5e956bff10a9540b06c856a633f42fff23307604edc5d5e95042b6823a4757f`.

Regular main now has a recipe for one of these thirteen scenarios,
native-session 10. Its four native tests cover a host change, a mode
change, a mode change during a pending goal instruction, and recovery
after a fresh model block. All four passed on the exact regular test
binary from `e6cd33c97f`. The stale operation could neither restart the
goal nor send a provider request. A fresh operation on the same thread
succeeded. The protected replay has SHA-256
`5dc6338e1a2fd4d4e4e33ae623b825db563ccf43074c313f783335dd554e2074`.
The frozen 0.170 candidate predates this recipe.

The protected runner also produced the actual CLI for `019cbd9f6b`,
SHA-256 `48907dabb5b1c1aa463b00bb67e3e179cf847365e2cf948c61ead82941c933ac`.
That executable passed the same five cross-login trials across 13
terminal processes. After a project moved, pressing Enter resumed its
original conversation under B and preserved the first 25,897 bytes of
A’s signed-package history or 25,911 bytes of its vanilla history.
The ordinary, duplicate-title and archived conversations also continued
as B. The combined login record has SHA-256
`8051a814b538f7f9f77af13f212d6d1ac695f48fcd9bc3bac57227787cdbc94b`.

The complete local review layout now pairs that CLI with the
Code Mode host selected by the protected compiler on the same source.
Its host SHA-256 is
`5c40ff0d62a8747c7ed14787d11a141bde4dafd3d0f6c901048225c9e47a760c`.
The native stdio test preserved session values, forwarded delegated calls
and controlled cells. It passed again against the named layout and failed
when its host executable was missing. From that layout B used bare resume
to select A’s original signed 0.11 conversation, restored the original
directory and authorized both new requests with B’s credentials.
The combined user-command receipt has SHA-256
`11f351e9dad1cc57ac1bb35328036aa1d82e7b72272a98984267b227b147a5f3`.

The same source-selected CLI passed all 18 direct comparisons against
official 0.170.0. Its report matched the `d217fa9da3` report on every
comparison, including both additional debug commands and all 174
attributed schema differences. All four complete provider catalogs matched.
The 18 app-server processes exited cleanly after 48 loopback requests
that compared model selection, tool placement and replay order. The
exact-source comparison has SHA-256
`763e41354cac47113869e5eab7f31f4a0ca93aad192bc48d319f78832fab4036`.
[Tagged test correction](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-019cbd9f6b)

On the earlier frozen source `e5c6a5fe13`, **22 of 78 scenarios have passing release evidence**.
The protected runner on a second Mac passed 121 named tests across seven
stages, including all 111 required names for build-and-adoption scenarios
1, 2 and 4. They cover compiler and source identity, hook loss and rollback,
upstream review admission, and rejection of invalid certification evidence.
Two readers reproduced those first three passes. Their initial portable
record has SHA-256
`f57c6bcee3463642e033b2e81ce7318f716529f7c1a687625dfa48c8f2147288`.

The native login suite then passed five named controls for account-isolation
scenarios 3 and 5. Native transport and WebSocket tests established
provider-recovery scenarios 3 through 19. Two independent readers accepted
all 22 scenarios and listed the same 56 still required. The combined
portable record has SHA-256
`600995aba324273d76939528ca1e085a9f42b45d3b949bcad7792c951da3664c`.
The 56 comprise 43 with registered recipes and 13 requiring new recipes.

A separate, non-promoting package attempt on this exact source passed the
35 GiB admission threshold with 37.5 GiB available. Trust 15698 refused 66
in-process procedural-macro dependencies of `codex-cli` and reported one
unknown obligation. The package command exited 1 with no artifact. The
release tool also confirmed that the candidate was outside private main and
that ALab 0.12.0 was already assigned to OpenAI 0.169.1. The package record
has SHA-256 `d2cd9e6e2d719b814abd2e1e708636eb36a7e92a961f172878b09e19887e71d9`.

On `d217fa9da3`, the non-promoting package tool independently resolved
the official tag and pinned Trust 15698. Its compiler identity probe passed.
The ordinary disk check then reported 30 GiB free against 35 GiB required
and stopped before application certification. The output directory was
empty. The exact package record has SHA-256
`9dafc60cf68930511433fa6fb62b8cdece06975b2e44d76267f662f15d45ba5d`.

The same independent package check on the test-corrected `019cbd9f6b`
authenticated the official release and selected Trust 15698. It found
24 GiB free against 35 GiB required. The source stayed clean and the
package directory stayed empty. Application certification on this source
is pending. The package record has SHA-256
`dce36836a40bc44f2ae812781b517c26331ab69bbffd2e14ba1311c6f849ba6b`.

On a second Mac the same source-authenticated package had 904.9 GiB
of disk space. Its 16 GiB compiler-identity reservation was still queued
after 61 seconds. The scheduler reported 0.4 GiB eligible after older
reservations. The wait was cancelled and its claim was released. The
package output was empty. The remote package receipt has SHA-256
`f7301c6240bee123f05ae122d6db331ffecf3f539dddb3d6a9bac9aa92133816`.

A second Mac had sufficient disk and authenticated the pinned Trust compiler.
Both the exec scenario prerequisite on `e1a564a9a2` and the non-promoting
package on `a248f110db` needed a 16 GiB build reservation. Neither was
admitted. We cancelled the waits after 19 and 5 minutes and verified both
reservations were released. Neither compilation started. The execution
record has SHA-256
`698673541e8789a282ed9d99a6948bc1392f0e7c130cdc9914f1ce24b0220510`.

[OpenAI development changes](https://github.com/openai/codex/compare/b0ecad9cf9c08318d79e9d7f91520e609d1c4d1a...99870c4f4921ee03f820e8e322ef97ed671ea6ef) · [Fork feature comparison](#what-the-integration-delivers)

### Remaining release work

1. Complete exact-source qualification of `cf8ec96860`, the source of
   the corrected review CLI and matching Code Mode host. Re-run the
   registered checks on that source, including transport and account
   isolation. Keep the earlier `019cbd9f6b` checkpoint of 63/78 as a
   comparison. Its 23 provider-recovery scenarios all passed. The
   suspended-input correction from that source remains in the new one. Add
   execution recipes for the other 13 contracts (three account isolation,
   six exec handoff, four native session). The new exec test captures an
   active review, a grandchild and identifiable queued input in one suspended
   snapshot. Its first native run on `e5c6a5fe13` compiled and reached the
   assertion on queued user input, which failed. A diagnostic rerun on `cd16b32def` compared the actual live and decoded
   queues. The original grandchild had one named user message with acceptance
   order 1. Its decoded suspended frame had none. An independent replay found
   that message in the original grandchild’s completed history. In the
   default mode, steering input can interrupt an open response stream and
   become completed input on the original turn between the two observations.
   The three-rollout record has SHA-256
   `da2ca945f1da42573d4d1355a0171ac48a45589d10a87f3e1952edf3568a0d58`. The corrected fixture holds the provider before sending response headers.
   It checks the full live queue, the suspended tree, original persisted
   history and exactly-once input delivery. Regular source `ca4c94a57e`
   passed its protected Core test in 12.78 seconds. All four Rust files
   involved are test-only. The independent verification record has
   SHA-256 `9dae823027283e551f091420f81b86191ff0056522056493d5c01dbacb2a0d79`.
   The original and diagnostic
   logs have SHA-256 `241f20d351c05f537f390c415e07d8797191a77c96dbcbeb63f371dd73721c03`
   and `4ee2ca6783bf5ae54ce0a72ae3d4a2faaabd2a4fe782c490990299834553d8d3`. The account
   race needs an idle conversation, A’s background refresh held at the
   issuer during a named account restart, and B’s first authenticated
   read after the restart. A new repeatable test established the first
   half on both review binaries and official 0.170.0. After an actual
   tool-using turn finished, A’s idle terminal requested an ordinary
   OAuth refresh through `/status`. Holding the local issuer’s reply kept
   both credential files intact and sent no new inference. Releasing the
   reply replaced A’s file atomically. B’s file stayed identical. The
   combined named restart and B’s first read still need the isolated
   debug terminal host and its matching lab controller. The two-binary
   receipt has SHA-256
   `67c7523e33ab9009a57103d0c2b6a17d9f62ba45ea7ff1c3d459f1e19b646ae5`.
   The separate run on `d217fa9da3` has SHA-256
   `b5fb5119868d77effc1438c5f19416474b92a7280793160a08012734aaa3d95e`.
   [Idle refresh test](../smoke/idle_account_renewal.py)

   The current isolated host can exercise plain Codex restarts. Its named
   restart controls have been exercised with another harness.
   Account scenarios 6 and 7 need a live retained-session receiver.
   Scenario 7 also needs the old frontend to call its reader-retirement
   routine on departure. The three-scenario audit has SHA-256
   `580ada37d47675a09cd9093d1e462624039b995e2a4d907dcdf6f9d31baf3c10`.
   Exec-handoff scenario 5 needs the same successor to restore both native
   goal accounting and the original terminal input. The current tests separately
   capture an active goal and partial paste, restore a seeded goal, and replay
   quarantined input. The exact-source coverage audit has SHA-256
   `86c195cc5945f0df27000a9491c7698df7dd1472fbcfce9e170957ed4fd4b083`.
   Four native-session cases also need distinct observations. Case 1
   needs a new process ID after a claimed CLI crashes. Case 9 needs the
   exact qualified package and host for the three-session quota test.
   Cases 10 and 11 can run in isolation. The former must change interface
   mode during a pending instruction write. The latter must close a real
   channel with a queued permit and observe a subsequent live turn after
   each refusal. The native-session audit has SHA-256
   `87f4d5bd7c096421d93336aab297f03385daf5f59319de281628a257d7df1536`.
   The existing hold on retained-session activation remains in effect. [Account race evidence](account-refresh-persistence-2026-11-19.md#assignment-during-an-outstanding-refresh)
2. Obtain an authenticated compiler that certifies the CLI and its full
   dependency graph. Trust 15698 refused the procedural macros. The newer
   signed Trust 27009 also failed its separate adoption check on the
   `anyhow 1.0.103` build script. On November 19 the authenticated macOS
   ARM64 index still selected 27009. We inspected 74 subsequent Trust
   commits through `6cd7c47262`. The compiler module has the same Git
   blob at both ends, and it still rejects required verification for
   build-script execution. At 17:32 UTC the signed ARM64 index was still 166, Trust was still
   27009, and 237 further Trust commits through `59a58b77a1` had preserved
   the entire compiler policy module. That independent record has SHA-256
   `7ef5bbc9269297f86c529f02e3c61fbd809bcc3001427617efdbf7da4bc24737`.
   The earlier signed index and source audit has SHA-256
   `820a35f98e077407d963cabf2ebc0b0433074449bb221cfca892f6ffb4f24378`.
   [Trust eligibility check](https://github.com/alabsystems/trust/blob/6cd7c47262c303c7624d64723ddbb702bfa273e6/src/tools/targo/crates/targo-core/src/compiler/mod.rs#L4855) · [Signed package index](https://github.com/alabsystems/aterm/releases/tag/atpkg-index-166)
   At 00:54 UTC on November 20 the signed arm64 index still selected
   Trust 27009 and Codex build 2026111701. The corrected review retained
   the pinned Trust lock, dependency lock and proof policy from the
   earlier packaging attempt that refused 66 in-process macro dependencies.
   The newer Trust compiler policy at `791a6f01418e` still excludes
   execution of build scripts from the required proof. The local volume
   had 26.41 GiB free against the 35 GiB cold-package admission floor.
   The signed-feed and compiler review has SHA-256
   `7c9c2accacf66a3c704fc550c5d77fa1dc55ceb0b4051610fb15b428e9f1bdf4`.
   [Trust compiler policy](https://github.com/alabsystems/trust/blob/791a6f01418e50ce56473eed6af31c094307a9bc/src/tools/targo/crates/targo-core/src/compiler/mod.rs)
   The unified `23a3833888` has the same 1,499 locked package identities,
   CLI manifest, Trust 15698 pin and dependency-inclusive certification
   policy as `e5c6a5fe13` and `019cbd9f6b`. Its TUI tests added one
   dependency edge to the already-locked `terminal_size 0.4.3`.
   The 66 procedural-macro refusals came from the actual certification
   on `e5c6a5fe13`. The local `019cbd9f6b` attempt reached the
   35 GiB entry check with 24 GiB free. At 05:43 UTC on November 20 the
   isolated package run on the exact `23a3833888` source selected
   Trust 15698 and completed its compiler identity check. With the
   ordinary 35 GiB requirement and zero-minute wait, the controller
   reported 21 GiB free and exited before application certification.
   It produced no package or proof report. The package receipt has
   SHA-256 `ece806c627d704d3f6237b40d2be4b58cab737a6ad713e7ece566c36e3d65e98`.
   Its release-ledger eligibility check also rejected the review
   commit because it is outside private main. The source and eligibility
   record has SHA-256
   `7c78863c340895ce12df56990a55fe104e7c62511dbcdc415aa317530294112c`.
3. Advance regular main to the reviewed 0.170 baseline through the normal
   upstream intake process. The production publisher checks that its
   source is on `origin/main`. Set the client-version floor to `0.171.0`
   there and claim the next ALab version through the release tool. ALab
   `0.12.0` already has an immutable ledger row for OpenAI `0.169.1`.
4. Run the complete gate on that exact shipping commit, build and sign
   its package, then repeat the cross-account terminal trial with the
   signed executable. Success means B can pick A’s thread by running
   `codex resume` after an ordinary managed update.

The unified source `23a3833888` adds 3,105 tracked paths, changes 714
and removes 477 against the exact OpenAI 0.170 tag. All removals match
its 49 explicit removal rules. Of the additions, 1,728 are under fork
Rust crates and 1,356 are under `alab/`. Against the newer OpenAI
0.171 tree the figures are 3,134 additions, 2,110 changes and 699
absent paths. Those totals include 222 paths outside the fork's removal
rules which the older target lacks. Path counts include code, tests,
schemas and tooling. The comparison receipt has SHA-256
`bce6ac15924157ebc6f0b8021a208968feee21be15427e6d4eaff5ea584d663a`.
[Tagged upstream comparison](https://github.com/openai/codex/compare/rust-v0.170.0...rust-v0.171.0)

Compared directly with the released OpenAI tree, the corrected review
source `cf8ec96860` adds 3,049 tracked paths, changes 711 and removes 477.
Of the additions, 1,713 are under the fork’s Rust crates and 1,315 are
under `alab/`. The complete fork registry has 15 feature families and 100
declared scenarios. Nine families and 78 scenarios apply to this gate.
The exact source comparison has SHA-256
`1139fc91580a48f8cb1ac0f368309f479a8342cc89b9d3f639c06d43470fa835`. All 477 removals match the fork’s 49 committed removal rules. These counts include
tests, schemas and release tools. The candidate adds 15 Rust workspace
crates under `codex-rs/alab-*`. Across the two CLIs' 77 help pages, the
only command-surface additions were `codex debug aterm-model-policy` and
`codex debug packaged-defaults`. Reproduce the path totals with
`git diff --no-renames --name-status 29eb79e986 cf8ec96860`.

At the 18:53 UTC comparison, 447 OpenAI main commits lay beyond the
regular fork’s integrated base `ce3031018e`. The separately tested
development candidate `1ea6e85e99` covers 427 of them. It also passed
18/18 direct comparisons with vanilla 0.170.0 and the seven cross-account
terminal trials. The development record has SHA-256
`722b1a515b6ff0828941b9f936d8d21786afb527851ac677caf3d4a5c9409cda`.
[Development candidate](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-1ea6e85e99)

A cumulative Git merge probe from that development candidate to each of
the seven later commits found two generated protocol schema conflicts from
the first commit onward. At the sixth commit, the app-server client manifest,
client library and turn processor also conflicted. Two further probes at `ab00c6d640` and `76aa618787` found the same
five paths in conflict. The new two-probe and release record has SHA-256
`07f90bf7eb5b60ad402f494ffcec9033d8904409c7ccd4501cc4e0c0a61cdc78`.
The seven-probe record has SHA-256
`7e327f3a1732123d14c5df98f69de0e13a372134a158920dabcdeaf5d5f74fd0`.
[OpenAI mailbox admission change](https://github.com/openai/codex/commit/0b248182be)

At `308ab1b895` the cumulative merge probe reported eight paths. Two are
workflows under the fork’s committed `.github` removal rule. Six need
reconciliation. Regenerate the two protocol schema exports and integrate
the app-server client manifest, client library, turn processor and
`codex-rs/core/src/unified_exec/process.rs`. A fresh ancestry and intake
check confirmed that the tagged candidate includes all five shared
mainline changes to the release branch point and all four release-only
changes. The release leaf contains zero later OpenAI main commits.
[OpenAI changes after the previous observation](https://github.com/openai/codex/compare/325123ae4a72faedf66939795bd0affd5c3939dd...308ab1b895229dbd258167cdbc97f450fca3d741)

A private rehearsal resolved the unified-exec hunk by recording the
original child’s exit before OpenAI’s atomic state update wakes the
waiters. This preserves concurrent failure details alongside the fork’s
shell completion evidence. The new upstream test also needs the optional
shell-log argument supplied as `None`. Two independent Git reconstructions
produced the same resolved production blob, `bd641f8675`. The companion
test blob is `31ba70b66a`. Native verification on the integrated
development source is pending. The rehearsal record has SHA-256
`c5c392a0501c4c3a7ff006f37ec15d1e0f3878d26e7db3c59009e4a22186ddb2`.

By 18:53 UTC five more OpenAI commits had landed, ending at
`99870c4f49`. The cumulative probe against the same 427-commit ALab
development review now reports ten conflict paths. The two removed
workflows still match the fork’s removal policy. The eight remaining
paths include two new conflicts in `codex-rs/core/src/session/mod.rs`
and `codex-rs/core/src/tasks/mod.rs`. The stable tag and preview
release stayed at 0.170.0 and 0.172.0-alpha.2.
[Five newer OpenAI changes](https://github.com/openai/codex/compare/308ab1b895229dbd258167cdbc97f450fca3d741...99870c4f4921ee03f820e8e322ef97ed671ea6ef)

The regular main executable at `ccfc6a95f9` separately passed four wire
checks and three terminal trials with its committed observer. B found A on
first login, after restoring the history index, and after A reverted a turn.
The three-trial record has SHA-256
`ccb18711426aa885c65227c03aaef879bd7bffa389de7ffe0995c9ce6ad4f8d8`.

The release-tool follow-up `c82101a89b` makes the candidate's own vanilla
comparator read its official tag and fixes the gate's synthetic Git
fixtures. It changes six tool, test and documentation paths. All 12,826
other tracked entries, including the Rust tree and package build inputs,
match `4c4d6f489f`. Its own ordinary gate admitted all nine upstream
changes and requested 78 scenarios. Its comparator passed all 18 checks
using the previously measured `4c4d6f489f` binary. Its independent
tooling record has SHA-256
`b37ef1ddb5d87779ee4f9c1ee21d402e6961005661d11ee5da3f5fc4c3dfc01c`.
[Release-tool follow-up](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-c82101a89b)

**39/97** was a progress count for an earlier source. **39 of its 97 required
fork release scenarios across 14 features had passing evidence.** One
scenario can exercise multiple tests. The exact tag has a different set
of affected features, and its results are counted independently.

At 15:00 UTC, two independent gate readers accepted **32 of 78** scenarios
for `4c4d6f489f`. The latest 15 passes cover provider recovery. Their native
runs passed 409 transport tests and 57 WebSocket tests. Of the 46 open
scenarios, 32 have execution recipes and 14 need new ones. The 20 GiB
free-space guard stopped the login, Cloud, API-fixture and TUI groups.
After the Cloud run, both readers still agreed. Eleven open scenarios
have a recorded resource refusal and 21 await execution. The final
handoff has SHA-256
`abd9da7689bd926ff37b38bf9146968a1f707de11b56e62b316a8dcf2fc1675b`.

The release collector can now recover completed scenarios from a native
run that stopped in a later stage. On the original tagged run, six
completed stages established build-and-adoption scenarios 2 and 4 before
the hook controls failed. Scenario 1 stayed unresolved. Both independent
readers accepted those two results and counted 32 of 78 when combined
with the existing evidence. A login run that stopped at the capacity
check yielded no completed scenarios. The recovered part has SHA-256
`3ad98c345db6b1d9beea3b8dd23dcca259dae2ddcb6cd288f5c987f3a74af0d1`.
[Recovery command](https://github.com/alabsystems/codex-alab/blob/23efe767123b5c6bfd21f07f791b32ef4a481b07/alab/design/upstream-alignment.md#L424)

The tagged follow-up `4893d713e7` puts the scope of bare `codex resume`
on the first help screen. In an 80-column terminal, the explanation
previously started on line 85 of short help and line 129 of long help.
It now fits on lines 1–8 in both. Both help forms also fit the scope on
the first screen at widths of 40, 60, 100 and 160 columns. Three CLI
unit tests and four runtime checks passed on this source. In seven fresh
cross-login trials all 18 bare-picker terminal runs exited normally, and
every continuation used B’s credentials on A’s original thread. The six
tool-bearing trials preserved A’s completed tool output exactly once.
Their Code Mode helper came from the prior candidate and has identical
source and dependency lock inputs. This follow-up requires its own
complete release gate. The cross-login evidence has SHA-256
`64ef3bf7ae428023ce8d26cd56d1d5c906e8de68a16a9d87f999fad41c611fe1`.
[Tagged help and cross-login source](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-4893d713e7)

The same executable passed all 18 direct vanilla comparisons, including
77 help pages. Nine provider catalog comparisons matched across 18 real
app-server processes. The two executables completed 32 turns over 48
loopback requests, with no requests to external providers. In an initial
control, the resume test fixture declared version `0.0.0` and the comparison
correctly failed the executable-version check. The passing run used the
version from the candidate’s committed Cargo workspace in a separate test
layout. The vanilla and provider records have SHA-256
`26dd131f0d892e66fc5e315d38df9f932467c6354fc2c782856339ce2df65dd3` and
`bc06a598f912cca6b2e37492518ff33e1d3fcf311ed77f2dae6f570d07827cfa`.

The ordinary 4893 gate admitted all nine upstream changes. Its first
scenario group stopped at 19.98 GiB, before producing a portable result.
The non-promoting package command selected Trust 15698 and stopped at
19.96 GiB, before certification started. A one-line release follow-up
`a248f110db` sets the committed client version to `0.171.0`, which the
packager requires for 0.170.0. The earlier development package already
computed this client minor. On the corrected leaf the release check now
reaches the production requirement for a commit on `origin/main`. Its
Rust tree, compiler lock and version ledger match `4893d713e7`. Its own
protected gate admitted all nine upstream changes and requires a fresh
evidence bundle for the nine affected features. The runtime and scenario
results above identify their respective sources.
[Client-version correction](https://github.com/alabsystems/codex-alab/tree/alab/release-0.170-review-a248f110db)

The seven-terminal and 18-comparison record has SHA-256
`f9f468c88668ed044b0a56db4033314055d14113fa207c9d39a66d8ea0ac6c00`.
The ancestry, provider, large-home and Code Mode record has SHA-256
`1c89e0468fbf9d65141fcaf47052be8b27e320807602602d0dca43b53146bc96`.
The exact tagged package refusal has SHA-256
`e79bc667760ccdb4259ce7fb8343ffe50d39ddeedc3adc7150d2230acff90053`.

## Earlier compatibility checkpoints

The following measurements identify the source and inventory used at each checkpoint.

**OpenAI stable is 0.170.0. Signed ALab for macOS ARM64 is 0.11.0**, built
from `70d6391259` on upstream 0.169.0. The 0.170 compatibility review
integrates 118 upstream commits through `27e2227b2b`. Of those, five
reach the official release branch point and 113 landed later on OpenAI
main. The released tag `29eb79e986` carries four commits on its own branch.

The stable review carries the upstream Bedrock incremental-tool default,
Mantle commentary replay, the Sol and Astra 6.2 catalogs, and the TUI
and protocol changes reviewed below. ALab adds named-account assignment
and renewal, cross-account local resume, aterm Autopilot, saved goal
and process recovery, and model migration. The development review
includes 427 upstream commits through `b0ecad9cf9`, which adds managed
Guardian policy refresh across threads. At 13:20 UTC on November 19,
OpenAI main was two commits further ahead. They expose Daybreak
availability in model discovery and add telemetry for shell-command
latency. Its latest stable release was 0.170.0 and its newest preview
was 0.172.0-alpha.1.
[OpenAI stable release](https://github.com/openai/codex/releases/tag/rust-v0.170.0) · [Development changes since the reviewed commit](https://github.com/openai/codex/compare/b0ecad9cf9c08318d79e9d7f91520e609d1c4d1a...39c29c3bb620f91e84a3bb266bb496dfb697c101)
[Feature comparison](#source-gaps-and-acceptance-checks) · [Release plan](#landing-sequence)

**Delivery plan**

1. Cut the shipping candidate from the fork's pre-release-branch-point
   history, merge OpenAI's five changes to the branch point, and then
   merge the official 0.170.0 tag. Keep its source and measurements
   distinct from the later mainline comparison above.
2. Run the vanilla CLI and provider comparisons on that exact source.
   Exercise no-ID resume from B's project with work created by both
   OpenAI Codex and ALab. Check B's outbound identity, A's surviving
   history and the directory where the resumed shell actually runs.
3. Complete the applicable fork scenarios, pass the source-selected
   compiler certification, and verify the signed package through the
   normal update channel. On `d7457d0cf1` the packaging check reached
   Trust 15698, which refused 66 procedural-macro dependencies and
   emitted no release artifact.

The official-tag source now exists at `93510f4379`. It includes all
five upstream changes to the branch point and all four release-branch
commits. It excludes the 113 later changes present in the compatibility
review. The ordinary gate on this leaf chose the shared mainline branch
point and requested 70 scenarios. Evaluating the official tag also
requires eight model-discovery scenarios, bringing the release
total to 78. Both counts refer to this exact leaf.

Run **`codex resume`** on review build `d7457d0cf1`. On the installed
signed 0.11.0 build, use `codex --no-daemon resume <thread-id>` after
switching accounts. The reviewed picker lists active conversations from all logins
and projects in the same local Codex home, newest first. Select a row or
type part of its title or preview. If the picker asks where to work,
choose **Use session directory** to return to A’s project. B continues
A’s original thread using B’s credentials, even while A’s daemon
stays connected. All 18 direct comparisons against official 0.170.0
passed. A further trial created A's conversation in the official OpenAI
0.170.0 binary. B continued the same thread with its tool results intact
and ran a new shell command in A's directory.

Two additional terminal trials exposed gaps when rebuilding the local
history index. On `7c06230400`, opening the picker while only a rollout's
header had arrived left its preview empty even after the complete file
was restored. A second trial reverted A's second turn and removed A's
index entry. A's surviving turn remained on disk, but B saw only B's
session. Both cases passed on `d7457d0cf1`. Once the rest of the file
arrived, B saw A in a fresh picker. After a revert, B's first resumed
request contained the retained turn and excluded the discarded prompt
and answer. The seven terminal trials, including renewal, a running A
daemon and the vanilla-created conversation, have combined result
SHA-256 `297aea5d985644e6e9164c0a6da2a5d2910187c413d4f599a691713475b47f58`.

**What 39/97 meant.** On one frozen source, 39 of the 97 required ALab
release scenarios across 14 features had passing evidence. A scenario can
require several tests. The older stable integration `bbd706f710` eventually
reached **75/97**. The release inventory on the reconciled source has **98**
scenarios, including a new check that an external editor keeps the draft.
The test-only successor `8bf920a93c` has independently replayed evidence
for **28/98**. Both release verifiers name the same 70 outstanding
scenarios. The 18 direct comparisons with vanilla measure CLI
compatibility separately.

The compatibility source reconciled with fork main `72a5986a47` has
**99** required scenarios. The additional scenario recovers local TUI
input that was waiting for delivery with interactive review turned off.
Its build and adoption checks run against this source's own inventory.

On the reconciled executable `d7457d0cf1`, two independently prepared
homes each held 10,001 conversations. One lacked A's index row and the
other had a row with an empty preview. B found A's oldest conversation
in 2.84 and 2.71 seconds respectively. The picker preserved all 10,001
history files. Every field in B's and the other 9,999 existing rows
was unchanged. In the stale row it restored only the two missing
presentation fields. The same executable matched the released OpenAI
provider catalogs and Bedrock behavior over 48 loopback requests.
The combined record has SHA-256
`143092a8b6498c2e7946ca6842c0a9294b2a9398d637cd6999c3905a70d13fc8`.
The regular fork implementation at `4120826544` passed seven protected
picker tests covering missing and blank rows, reverted history, rejected
ancestor copies and pagination. [Compatibility review source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-d7457d0cf1)

### Compatibility candidate at 7c06230400

Stable review `7c06230400` repairs a stack overflow that appeared when a
session resumed model discovery after an idle handoff. Two existing Core
tests exposed it on `71963b1805` with the normal 8 MiB worker stack. The
repair heap-allocates the provider request future before the opening observer
wraps it. Six functions on the observed call stack together reserve 5,095,984
bytes on the repaired binary, down from 7,007,328 bytes. All 14 handoff and
retirement tests passed on the stable revision and the corresponding regular
fork commit `74207b61d0`. [Stable review](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-7c06230400) · [Regular fork fix](https://github.com/alabsystems/codex-alab/commit/74207b61d09f72e31e1211fe7fae2b73940513c4)

The same repaired executable passed all **18** direct comparisons with
OpenAI 0.170.0 and **five** bare-resume terminal trials. A created each
thread with signed ALab 0.11.0. B continued it using B’s own credentials
on a first login, from page two, while A’s background service remained
connected, after recovery of a missing history index entry, and after B
completed renewal of its expired sign-in. A separate home held 10,001 conversations;
search found A’s oldest thread in 3.21 seconds. All history files and all
43 columns of the existing 10,000 index rows were preserved. The three
release-branch behaviors in the comparison below also matched on this
executable over 48 loopback Responses requests. Live provider entitlements
remain unmeasured. The combined acceptance record has SHA-256
`5887dd82b089d22a0c21e3f14dfbfe0a9f6dd692a3790cb19c54afb3470d8bab`.

**Shipping status.** All 118 upstream intake decisions passed. Independent
replay confirmed the first three release scenarios on the repaired source.
The ordinary feature gate requires a complete 98-scenario bundle. The
separate package build stopped when Trust 15698 could not certify 66
procedural-macro dependencies. Signed index 166 still offers ALab 0.11.0
build 2026111701. Its newer Trust 27009 failed the candidate compiler
adoption check on the build script for `anyhow 1.0.103`. The package
attempt and signed-index check have SHA-256
`fff24c681dc57ed6c78310ae787b4510a11754cc0e1199eff4718083d1975294`
and `21303e8e49315e9bae34e0d276f2b5c181447ca1f0b300decf453f089786cd84`.

On the installed signed build, B tried
`codex --no-daemon resume --all --include-non-interactive` in another
project after A created the only local conversation with `codex exec`.
The picker showed **No sessions yet** (0 of 0). The same signed app-server
returned A’s original thread and project when asked for all local owners.
Use `codex --no-daemon resume <thread-id>` on 0.11.0. The terminal and
server comparison has SHA-256
`99d1d3aa5ec11882b921dd884311a4cc1d53eea33e0583eb67b32dc4fb0d3cf8`.

### Provider recovery on the test-only successor

Stable revision `8bf920a93c` corrects one test initializer for the
provider’s model-not-found error. The previous initializer prevented the
codex-api test target from compiling. All 409 tests across the three
transport suites passed on the correction and on regular fork commit
`5ee41da69a`, including the test that originally failed to build.
Production source is byte-identical to `7c06230400`. The five terminal
trials and the vendor comparison above identify the executable they used.
[Stable correction](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-8bf920a93c) · [Fork correction](https://github.com/alabsystems/codex-alab/commit/5ee41da69a28e35a8ee06b5c98f05c058dcb6fd5)

On `8bf920a93c` a fresh gate admitted all 118 upstream decisions. Two
verifiers replayed **20/98** release scenarios from newly executed
tests, comprising 17 provider-recovery and three build-and-adoption
scenarios. The portable record has SHA-256
`95c036259237dd659411550f57982675a0d09adb07b86ba082115231195d5908`.
Five native-session and three qualification-fixture scenarios brought
this source to 28. The 28-row record has SHA-256
`c639ead12b505332614ba18a32fcb45d63637d345d5143853e42a889a3d919d6`.
The latest package attempt on this revision stopped at the 20 GiB native
disk floor while waiting for 35 GiB of packaging space. The last
completed certification attempt on the preceding revision is recorded above.

On `7c06230400` all 19 model-discovery wire tests also passed through
the ordinary application, including four cases that continued the
original live shell. The wire-test result has SHA-256
`798285182cfe348ead32a1f639a70dd9def7dd2a10125e23f8c6bce3715fd136`.

### Development comparison through the Guardian refresh

Development review `a42dc04687` reconciles OpenAI’s new thread registry
with ALab’s private-parent registration and live-upgrade inventory. All
427 upstream intake decisions passed. Eight native controls exercised
policy refresh, cancellation, private parents and upgrade ownership.
Two further cloud-config tests confirmed that the thread manager reports
when a live thread rejects the update. The new loader is installed by the
test hosts on this revision. Production policy delivery remains unverified.
The CLI and its matching Code Mode host passed the gRPC integration, all
18 direct checks against OpenAI
0.170.0, and five separate cross-login terminal trials. The combined
development evidence has SHA-256
`b2431e38e88b559ac91f531fd5abefd24543c73a3f35e1ad9af4f4a85890e133`.
[Development source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-a42dc04687)

### Reconciled 0.170 baseline

Private fork main landed the resume fix in `999bd92ace`. Stable review
`71963b1805` incorporates that commit. The merge preserved all 115
categories of fork hooks and passed intake for all 118 upstream commits
in its range. The ordinary gate then requested the release evidence for
98 scenarios across 14 affected features. The first three passing rows
cover build and adoption. Seven protected stages executed 111 distinct
named tests for those rows. Their portable evidence record has SHA-256
`88114d5f5e3c87c7b34aae7dbcf944fbb2949b0fb2ab385010031de706b3efdc`.
[Reconciled stable source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-71963b1805)

Two compatible collectors between them have runnable recipes for 80 of
these 98 scenarios. Eighteen require additional evidence, including
real process replacement, acknowledgment loss and retained credential
reader retirement. Both collectors matched every ordered scenario
description on the release candidate. Their execution plan groups
related tests and records which scenarios have a passing predecessor.
The 98-row inventory has SHA-256
`e74c5c3559af5f3cdeb1ed3453e88d9d0f16e697c93ea2a93a3e1c2ed675e3db`.

This CLI passed all 18 comparisons with official 0.170.0 and four
separate terminal trials. B resumed A’s signed 0.11 conversation from
another project on a first login, from page two, while A’s daemon stayed
connected, and after restoration of a missing index entry. Both resumed
requests used B’s credential and included A’s earlier tool result.
The vanilla log and four-trial receipt have SHA-256
`e4acb4ca3131a955f098b11eb95bbcaeb4946d158493914796590be89583e401`
and `1473578d7755d8e735db27277fbb7a5fc68f0a38101e4074979119236af7b034`.
In a fifth trial, B had an expired named credential and no previous
conversation. B exchanged its token once, persisted the replacement and
resumed A’s original thread. A’s credential file retained its original
identity and bytes. The result has SHA-256
`760a0877e6e80cedb72a5427471ff1ff308003d24d0972d8031d1e9b0530cdf8`.

Three behaviors that shipped on the official release branch also matched
in a separate comparison of the two executables. Eighteen isolated
app-server sessions produced 48 Responses requests against loopback peers.

| Release behavior | Observed on both executables |
| --- | --- |
| Bedrock incremental tools | On all 36 Bedrock requests, Mantle and Runtime advertised the fixture tool in `additional_tools` by default and when enabled. Disabling the option put it in `tools`. |
| Mantle commentary replay | Mantle removed empty commentary annotations on tool continuation and on the following turn. Runtime retained them. Both preserved citations, reasoning and tool-result order, and final-answer annotations. |
| Sol and Astra catalogs | The full model lists matched for first-party (14), Mantle (11), Runtime (20) and Vertex (6). Actual requests matched for the default Sol and an explicitly selected Astra. |

Both the release tag and the candidate contain the same bundled model
catalog and all three cloud catalog blobs. The source and loopback
receipts have SHA-256
`c5aecb97969461e425546f60f68574e3f1eb2f8569f7c6a595cab4ce6b81652c`
and `dabbcafdfac94045f3ecaecce347ab5a238a930442c829e22b0d4bbc3bccc98e`.
The runtime comparison used synthetic credentials. Live provider
entitlements remain unmeasured.

Account-isolation scenario 2 covers a different transition. It requires
starting A’s refresh, switching the live assignment to B while that
refresh is held, and observing the new owner’s admission. On
`71963b1805`, a picker selection whose underlying binding changed before
native restart sent only the account label. The isolated host refused it.
With the managed owner unavailable, the peer saw zero token exchanges,
unchanged credential files and five requests to A’s own model and usage
endpoints under A’s original bearer. The authenticated assignment race
is still open. The sanitized receipt has SHA-256
`d18522de6a176eaf4b28d213504d64d62c7b912ead67c11273cad65c489460d3`.

In a fresh 10,001-conversation home, opening the bare picker restored
the oldest index entry. Searching for its title found the same A-owned
thread. All 43 columns of the 10,000 existing rows and all history file
contents stayed unchanged. The indexed listing grew from 400 pages to
401. The terminal result and row comparison have SHA-256
`b1d833d6ca5fb7893a28f42cc307a8ea2f42b0c38348ff1c2557c7590d58f19c`
and `693227b2f5197d760825801e8ab232da0d1eba9db72f83418fe0e8968dd75a83`.

Normal packaging on `71963b1805` reached its source-selected Trust
15698 compiler. It refused the CLI’s 66 in-process procedural macro
dependencies, reported one unknown obligation and emitted no package.
The compiler report has SHA-256
`ca011df03a7254cd595b287f4f35e93bd7314a3e6f8a84d488188d4456e0ad31`.

In a direct comparison, A created the only local conversation with signed
ALab 0.11.0 using `codex exec`. B then ran plain `codex resume` from a
sibling project. **Official OpenAI 0.170.0 displayed “No sessions yet”
(0 of 0). The stable-aligned and development ALab pickers selected A’s conversation (1 of 1).**
The official app-server could still retrieve A’s thread when queried
across accounts, sources and directories.
[Remaining scenarios and exact-source evidence](#current-evidence-on-the-reconciled-0170-source) · [CLI and protocol comparison](#measuring-public-cli-and-protocol-differences)

On frozen `bbd706f710`, the initial picker could miss a restored local
conversation. A’s signed exec history file was intact, but its SQLite
index row was missing. With B’s conversation indexed, the first picker
showed only B (1 of 1). An ordinary app-server listing that also reads
history files recovered A’s row. Reopening the picker then showed both
threads, and both history files kept their original bytes. The original
terminal and database result has SHA-256
`6972a861fd8e118988506b6266c176ec1d409ba08729e79f72161aee566a2d92`.

At 10,001 valid active history files, a fresh all-project list returned all
10,001 indexed UUIDs in 401 pages. Removing just the oldest index entry
made both list modes stop after 10,000. Moving one newer file aside let
the ordinary listing recover the unchanged oldest file. Keep the SQLite
ordering and pagination for existing rows, and recover absent rows even
outside the filesystem listing’s 10,000-file window. The scale comparison
has SHA-256 `c2cc68721f7eca08cee99d172a3fdd072c98b6884d99129b117842181b4106bd`.

**0.170 follow-on `890081896a` recovers restored conversations on the
first bare picker.** Before issuing its first local page, the picker
checks the saved history filenames against SQLite and adds missing
resumable threads. Existing rows keep their selected histories. B found
and continued A’s restored exec thread in its original project. A second
trial restored the oldest of 10,001 conversations. Searching by title
found it, while all 10,001 history files and all 43 columns of the 10,000
previously indexed rows retained their values. This follow-on also passed
cross-page search and first-time resume with an expired B login. The
four-trial receipt has SHA-256
`4f671e6f5fd020039d114f56126669d72d962c899cc502d3b05c4d9c3b9c492d`.
Its candidate CLI also passed all 18 comparisons with the genuine 0.170.0
release. In a separate terminal, B continued A’s signed 0.11 exec
conversation while A’s existing daemon remained connected to A. The
isolated test layout used the candidate CLI and signed helper programs.
The comparison and daemon receipts have SHA-256
`37672bd52c1cbf3b58dc2194e46f9af2a672421300feda137c972aa7011757e4`
and `62a1a7cfabda3d17ca5f68252e8cd7c7eec0d0b737f0c99772e38e9c4f87354e`.
Three native TUI tests passed on the same revision. They exercised
restoring a missing row before SQLite pagination, the fallback from an empty
first database page, and the normal end of a later page. The verifier
also rejected a missing selector. The three-test receipt has SHA-256
`2c774bd77b1924b3a4a713d378cf85717f0e0153102e4769746bab81beb09641`.
The 72-of-97 qualification count above belongs to `bbd706f710`.
[Stable follow-on source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-890081896a)

On regular fork integration `43672b9841`, all three native picker checks
passed. A fourth regression in `5382789b8c` reproduced the filesystem
reader’s 10,000-file limit with two real conversations and 9,999 newer
revision filenames for the already indexed thread. The ordinary reader
missed the older conversation at that limit. The picker’s repair made it
pageable under its original path and preserved the indexed thread. All
four tests passed together in 0.77 seconds. The native selection receipt
has SHA-256 `a539e4f831f8c39ba5fb5e528a1856a04e3da9da880d48e122230bc4cea57152`.
All 57 Python tests for the resume and vanilla comparison tools also
passed on regular integration `8305f076f4`.

The same `890081896a` package attempt reached the source-selected Trust
15698 compiler. Certification refused all 66 in-process procedural macro
dependencies for the CLI and reported one unknown obligation. The package
directory was empty. The compiler report has SHA-256
`e36d22274233afe001cac7e8cf60f49a09c365fee25449a135a29db460485067`.
The normal repository check also stopped before compilation. Against fork
main `b5789167b0`, this stable review was behind on five groups of ALab
hooks, covering defaults, exec recovery, interface, models and queue review.
The exact check log has SHA-256
`967634f528505cf44fe933948eea91cd57281d3ffb4c95e3a638c52fc418fd15`.

On the installed signed 0.11.0 build, use `codex --no-daemon resume <thread-id>`
after switching logins. On the reviewed build, run `codex resume` with no ID.
Both use the shared local history. [Cross-login instructions](../interface.md)

The next release needs passing evidence for all scenarios on the candidate
to be signed, and a Trust compiler that can certify the CLI’s 66 in-process
procedural macro dependencies. After both pass, sign the package and repeat the
vanilla comparison and cross-login tests on a managed installation. The separate
development review through OpenAI `d52a39b307` includes 426 upstream commits
since fork main’s last upstream merge. It passed all 18 vanilla comparisons
and four cross-login trials. An upstream test exposed a difference
when external credentials change during an in-flight custom-provider retry.
[Release steps](#landing-sequence) · [Development comparison](#model-promotion-on-current-upstream-main)

The official sync subsequently merged fork main `896b455794` into
`bbd706f710`. Its sole source conflict was in `core/src/session/turn_input.rs`.
The resolution preserved upstream’s extraction of dictation metadata
and ALab’s explicit input and manual queue predicates before destructuring
the request. The resulting file has Git blob ID `6553883bfa8c`.

On that reconciled source, the native gate still requests 97 scenarios
in the same 14 contracts. It stopped at feature-evidence admission with
check status 1 / sync status 4, before compilation. The feature
collector reported incomplete coverage for account isolation, exec
handoff, execution continuity, goal storage recovery and native session.
The gate log SHA-256 is
`41f355d84fbf0d289ebede0309c3e042711537dad0168c0377b7f1895901ec5a`.
[Reconciled release source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-bbd706f710) · [Tested cross-login picker](https://github.com/alabsystems/codex-alab/tree/alab/resume-default-0.170-1b1eb96b99)

The official reconciled CLI was also built as a native test prerequisite.
Its SHA-256 is
`ee0cf3bc742c45adb83f6ab769eacb157a50969ef83604972da6802067c40635`.
All 18 comparisons with the genuine 0.170.0 release passed. The comparator
examined 77 help pages and 26,145 shared protocol entries. In two fresh
homes, signed ALab 0.11.0 created a conversation using `codex exec` as
A. From a sibling directory, plain `codex resume` as B showed both
accounts' conversations on the initial screen, reopened A's original
directory and tool result, and authenticated both new requests as B.
A third trial put 25 of B's newer conversations ahead of A's. Page Down
loaded page two, and Enter on row 26 reopened A's original thread as B.
A fourth trial started B with no conversations at all. Plain `codex resume`
selected A as row 1 of 1. B continued it without creating a second thread.
The original two-account test also passed with A's managed daemon still running. A's existing
client stayed on A, and the daemon subsequently stopped and released
its socket. The isolated package used the signed helper binaries and
the tested candidate CLI. Its combined results manifest has SHA-256
`44aa9e6e5fe43f30580aafbee04f123ac879f502b3dc35e799b02e68e360bd19`.
[Receiver compatibility and interactive tests](managed-daemon-account-resume-2026-11-19.md)

A cancellation during a credential refresh exposed another difference in
account ownership. Fork fix `9eaaab58d8` holds the credential lock until
the background file write finishes. Its 0.170 comparison `dbd4e1f854`
passed all five login tests, all four execution-wire checks, all 18
comparisons with vanilla, and the signed exec-to-bare-resume trial with
A’s daemon still running. Its CLI SHA-256 is
`6ea0200a145bc45156333f6caacbe510b3279f05323a70aecd09772e3931014f`.
Its own no-promotion package attempt reached certification. Trust 15698
reported one unknown because it could not certify the CLI's 66 in-process
procedural macros. The command exited 1 and produced no package.
[Source-bound refresh and resume results](account-refresh-persistence-2026-11-19.md)

### Current evidence on the reconciled 0.170 source

The frozen stable source `bbd706f710` reached **75 of 97**. The 22
remaining scenarios include four for account isolation, eight for exec
handoff, five for native sessions, one for execution continuity and four
for goal-storage recovery. Three later collections verified native goal
teardown, attribution of goal calls and exec handoff. Four native
tests interrupted sampling after a committed goal, preserved the goal
and usage, and observed recovery exactly once. Eight Core tests checked
sampling counters, audio metadata, allocation ordinals, review budgeting
and malformed snapshots. Five native tests distinguished direct and nested
goal calls that shared an observed identifier, including reordered
callbacks and interrupted or failed commits. Rebuilding from all 28
original parts reproduced the portable record and its 75 captures.
Two verifier revisions independently reproduced the same count, and
full release admission identified all 22 missing rows. The archived
record has SHA-256 `48fee9dc32a5cad456e03bcc0540d3ae36ebf5a6a264d7afcfd9a1294b5e8582`.
The executable collector for the ten-scenario exec inventory is preserved
on a private branch. [Historical collector source](https://github.com/alabsystems/codex-alab/tree/alab/qualification-host-bbd-5f6d8f946f)

At the earlier 72-scenario milestone, the verifier read 25 parts against
one source, integration base and 14-contract inventory. Nine contracts had
evidence for every scenario. The table and archive below preserve that
milestone.

| Contract | Verified | Still pending (scenario numbers) |
| --- | ---: | --- |
| account-isolation | 3 of 7 | 2, 5, 6, 7 |
| exec-handoff | 1 of 10 | 1, 2, 3, 5, 6, 7, 8, 9, 10 |
| native-session | 4 of 11 | 1, 4, 7, 8, 9, 10, 11 |
| execution-continuity | 6 of 7 | 5 |
| goal-storage-recovery | 3 of 7 | 2, 5, 6, 7 |

The portable archive `codex-0.170-72-of-97.tar.gz` contains the evidence
record and 72 distinct content-addressed captures. Its SHA-256 is
`ec05db245ec4b90c2a78e03af0fa9f807938a93495d542e1fb881a20695fe5c0`.
After extraction in a fresh directory, the verifier reproduced the progress
report byte for byte, SHA-256
`be888554b0bf86d1b2ac3a28def475a12292f5882f8b774ff704ca63ed02bbb6`.
Submitting the same record to release admission reported all 25 missing
scenarios. Rebuilding from the original 25 parts with exporter
`a8068c640e` reproduced the archived record and all 72 capture hashes.
Repeating a part, including one that spells out the default Rust test
format, gives it no additional credit. The 18 vanilla checks and the interactive signed-session trials
have separate receipts.
[Export and replay commands](../design/upstream-alignment.md#validation-and-reporting)

### Reading the scenario counts

The read-only `alab/feature_evidence.py progress` command reports the
number of covered scenarios, the denominator across affected features, and
the outstanding scenarios by name. The earlier **39 of 97** meant 39
ALab scenarios had passing evidence at that point on `a98666774c`. Each
scenario counts once, even when it takes several tests to demonstrate it.
The 18 direct comparisons with vanilla are measured separately. Replaying
the first ten original parts reproduced **39 of 97**, with progress report
SHA-256 `cef7fb6fd2b545674d5031b69b80ba26d00ef24d1d78596301ceba18728ba24e`.
The eleventh part completed 23 provider-recovery scenarios, bringing that
same source to **62 of 97**. Supplying a part from that
revision alongside the reconciled source's evidence caused the command to
exit with an error. Repeating a part gives no additional credit.
[Progress command and example](../design/upstream-alignment.md#validation-and-reporting)

At 07:25 UTC on November 19, six evidence parts on the reconciled source covered **12 of 97**
scenarios across five features. Account isolation covers 2 of 7,
native session 1 of 11, execution continuity 4 of 7, build and
adoption 4 of 4, and goal storage recovery 1 of 7. At that point 85
scenarios still required evidence on this source. The progress
report has SHA-256
`222e42750418fc16060f7bd0674b12f3716efc3a12fe2f800371144387dc708e`.
The separately measured 18 vanilla checks and the two interactive
cross-login trials also passed on this executable.

The following scenarios were pending on the earlier review and have native
test evidence on the reconciled source `bbd706f710`. The source, inventory,
test names and SHA-256 hashes of each log were checked again by the progress
command. The managed reader and Cloud fixtures used isolated accounts and
loopback peers.

| Feature and scenario | Observation | Part SHA-256 prefix |
| --- | --- | --- |
| Account isolation 3 | After reader replacement reported ready, the retained HTTP client refused its next request. The peer counted only the first request. | `9ccf66c1574d` |
| Account isolation 4 | The Cloud client enforced its destination policy, refused a mismatched credential and transport, and stopped sending after its reader lost the source. The two independent readers acknowledged retirement after their clients were dropped. | `9ccf66c1574d` |
| Goal storage recovery 1 | A real SQLite full-disk error interrupted a write. The next borrower committed a transaction successfully, and an ordinary dropped transaction left the connection reusable. | `1db14dd49667` |
| Native session 3 | Mixed inbox entries kept older unread work visible. Ambiguous offsets, duplicated row IDs and malformed snapshots withheld acknowledgement until the snapshot recovered. | `1b9f4c1d4307` |
| Native session 2 | Five claim tests covered stale buffered proofs, release, concurrent close and reconfirmation. Three policy tests covered concurrent reads, invalidation and closure. | `65d3cc67bf34` |
| Native session 5 | Startup swept dead process records and handed them to the cleanup caller. It preserved a live child’s record and replacements published between inspection and retirement, including a replacement with identical known fields. Eight named checks exercised the production sweep against real files. | `5e43d018a38a` |
| Native session 6 | An accepted prompt continued only the root session targeted by its approved plan. Two TUI and five Autopilot tests covered a failed attachment, changed or missing preparation, and unrelated sessions. | `326fe9e89429` |
| Goal storage recovery 3 | Four retained-usage tests and a raw-replacement storage test accounted for older turns and quarantined descendant usage after a goal was replaced. | `e50033cac2ca` |
| Goal storage recovery 4 | A queued older stop could not reverse reactivation of the same goal. A held failed stop required a fresh, acknowledged action. | `e50033cac2ca` |
| Exec handoff 4 | Five Core checks and three app-server checks restored saved helper intent under the current account and model. An implicit choice stayed implicit. The original account delivered input once, another account refused an inherited choice and an explicit override survived. | `8fcccd00119a` |

A separate protected run on fork source `750238dbd1` exercised
account-isolation scenario 5. The fixture held a credential already being
formatted and a second credential awaiting output. After the native source
closed, both destination buffers stayed empty. The same assertions passed
after a managed account policy change, and a healthy control wrote its
expected test value. Two source-replacement checks and an account-retirement
check also passed. The four-test log has SHA-256
`1ca05e03e26e6e420069c6a0deea1f07c8cd812544ba53c80b2e1ff0b6384402`.
This coverage belongs to a revision that contains the new fixture.
[Native output fixture](../../codex-rs/alab-core/glue/auth_native_member_tests.rs)

Native-session scenario 4 was exercised separately on fork source
`2029213177`. Eight named control-socket tests passed, including independent
limits for STATUS and BYTES replies, line framing and the aggregate wire size.
The oversized-header cases returned before allocating the advertised body.
Two deadline controls passed in a separate protected run. The main and
deadline logs have SHA-256 prefixes `e1eda49a0785` and `56d05b2a814b`.
These new test cases belong to the fork revision that introduced them.
[Control-socket framing tests](../../codex-rs/alab-aterm/src/conn.rs)

Native-session scenario 1 still needs a process-replacement observation.
The existing controls rebuild a successor from durable files in a test
process, or renew a launcher by replacing the executable under the same
PID. The remaining check must witness a claimed CLI exit and a distinct
successor recover its record and claim under the same native session.
The aterm lab already observes two replacement process IDs on the same
terminal and conversation. Its available crash modes also activate the
live receiver under a human hold. A crash-only host selector and an
identifiable package for the tested source would allow an independent run.
The frozen test CLI lacks the package version the host requires.
[Durable record tests](../../codex-rs/alab-tui/src/record_custody_tests.rs) · [Native process observations](../smoke/native.py) · [Native host crash test](https://github.com/alabsystems/aterm/blob/a7d3bf5edeae0a32cd4e6fd7da240744db865f9c/crates/aterm-lab/src/codex_alab_proof.rs#L3345)

**Release plan**

1. Complete the remaining account isolation, execution handoff, execution
   continuity, goal storage and native session scenarios with the existing
   hold on full-capsule activation in place.
2. Run the complete feature and compiler gates against the source selected
   for release. The compiler must certify the CLI and its dependencies.
3. Sign the release, install it through the managed channel, then repeat
   the vanilla comparison and cross-login picker checks on the installed CLI.

At 09:10 UTC on November 19, the official latest stable release was
0.170.0 and the newest prerelease tag was 0.172.0-alpha.1. OpenAI’s
development branch reached `b9559ff737`, 422 commits past the fork’s
mainline frontier and 304 past the 0.170 review. The prerelease branch
point was 250 upstream commits ahead of the review. The literal stable tag
has four unique commits. The integration contains the three behavioral changes
for the Bedrock default, Mantle replay and provider model catalogs. The fourth
sets the Cargo release version. The review executable reports `0.0.0`.
Compared with the stable protocol, the review contains OpenAI’s subsequent
replacement of the experimental `currentTime/read` callback with synchronous
host clocks. Signed index 165 still
selected ALab 0.11.0 on this Mac.
[OpenAI stable release](https://github.com/openai/codex/releases/tag/rust-v0.170.0) · [Development since the review](https://github.com/openai/codex/compare/27e2227b2b...b9559ff737)

### Model promotion on current upstream main

The development build `0.13.0-dev.21056+codex.0.170.0.g56bc1f03a` includes all
422 reviewed commits through official `b9559ff737`. Its macOS ARM64 CLI SHA-256
is `865cf95ccd1f473d436e30058d9860929fb07709a6ab91bed8309616654eee85`.
Conversations created by signed `0.11.0` resumed on this build. When the loopback
provider rejected Sol 6.1 before delivering output, the same turn completed on
Sol 6.2 over HTTP streaming and WebSocket. When a reply had already reached the
user, it appeared once and the following turn used Sol 6.2. Explicit pins held.
Both `codex resume` in a terminal and `codex exec resume` independently completed
a refused turn using the replacement ID declared by their provider. All 19 model
wire tests passed, including using the original shell process after promotion for
Sol, Astra and provider-defined model IDs. The signed channel still serves 0.11.0.
[Exact integration source and tests](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-56bc1f03a3/alab/feature_recipes)

The development integration `56bc1f03a3` passed all **18 direct checks**
against the signed vanilla 0.170.0 binary. The comparison measured 77 CLI
help pages (14 attributed differences), 26,138 shared protocol entries
(1,142 attributed differences) and 60 shared home paths. Two upstream
changes accounted for initially unexplained results. OpenAI changed
`mcp_lazy_stdio_startup` from false to true and bundled four
`create-statusline` files. Comparator `50ea5a923b` verifies the observed
boolean against the stable tag and both source trees, and each installed
file against both source blobs. The full passing log has SHA-256
`b785810ad38049f0838036e92a5906273c53c408f8bb8796ad7b289050a4f804`.
[Upstream feature change](https://github.com/openai/codex/commit/637c429472) · [Statusline skill](https://github.com/openai/codex/commit/b47df356af) · [Statusline declarations](https://github.com/openai/codex/commit/7df454ceb5)

Three interactive trials on `56bc1f03a3` also reopened conversations made
by signed 0.11.0 with plain `codex resume`. They covered a newly signed-in
B with zero conversations, a 26th conversation loaded by paging, and a
live daemon still serving A. All three resumed the original thread in
its original directory and authenticated both continued requests as B.
[Source, commands and receipts](managed-daemon-account-resume-2026-11-19.md#cross-login-resume-at-the-openai-development-tip)

At 09:47 UTC, the managed macOS command still reported `codex-cli 0.11.0`.
Index 166 selected build `2026111701`, based on upstream 0.169.0 at
`ce3031018e`. OpenAI's release API returned stable 0.170.0 and preview
0.172.0-alpha.1, and its main reference returned `b9559ff737`.
[Stable release](https://github.com/openai/codex/releases/tag/rust-v0.170.0) · [Preview release](https://github.com/openai/codex/releases/tag/rust-v0.172.0-alpha.1)

At 10:01 UTC, stable and preview were unchanged. OpenAI development had
advanced another three commits to `6271f35f52`, 425 beyond fork main’s
upstream integration point. The changes send authorized connector results
over Responses WebSockets, move Code Mode host connections to gRPC, and
box the thread startup future. The last two affect the working path of a
resumed turn. The earlier `56bc1f03a3` receipts still describe the prior
tip. We tested the newer source with its matching CLI and Code Mode host
and examined the ALab account-assignment hook in the boxed startup path.
[Connector and Code Mode review](../upstream/336dc645fb-from-b9559ff737-intake.json) · [Thread startup review](../upstream/6271f35f52-from-336dc645fb-intake.json)

At 10:51 UTC, OpenAI stable remained 0.170.0 and development advanced one
commit to `d52a39b307`. Attached Guardian reviews now honor the configured
elastic context budget. Review `86972ec0b5` integrates this change over
the prior development review and its test-only repair. All four changed
files match upstream byte for byte. The intake check covered 426 upstream
commits from fork main’s integration point. All 72 tests in the Guardian
context library passed, including the fixed-budget and elastic-budget cases
for the exact pending action. The library log has SHA-256
`c6542a933fc7b63b793c7ec37d55f096ce74d2af5c34f1c2d05abef6295ec7cd`.
[Guardian change](https://github.com/openai/codex/commit/d52a39b307b77e81cd272d08189341d944542b82) · [Intake decision](../upstream/d52a39b307-from-6271f35f52-intake.json)

The source-matched CLI and Code Mode host on `86972ec0b5` passed all
18 comparisons against genuine OpenAI 0.170.0. Bare cross-login resume
also passed with a first-time B login, with A on page two, and with A’s
managed daemon still connected. Each continued A’s original thread and
working directory under B’s credentials. The full comparison log has
SHA-256 `3076d5a3ef47c56a83b2f0b36546cca46e9ea134e6ce3d670cced9738c56bfa6`
and the three-terminal receipt has SHA-256
`2198d7ecd8596e23a3a376817e32a342e3d79869de155ef43ed49ae52bd79adf`.

The real Core integration passed its request-history check. The consecutive
reviewer-reuse test stopped before its action when macOS denied a nested
sandbox process that reads the fixture’s project instructions. Reviewer
reuse in that integration therefore remains unverified on this revision.
The protected Core receipt has SHA-256
`273c018e9c97719abb7ee732339538e744fe54a2711bd988e4c366dc983c2bf5`.

Development follow-on `c9bb2bd018` carries the restored-history fix over
the same OpenAI commit and preserves the measured decision on in-flight
custom-provider credential changes. With `86972ec0b5`, the ordinary
picker omitted A’s restored file in the already-populated local index.
With `c9bb2bd018`, it displayed A on the first screen and continued the
original thread under B. The comparison used actual terminal processes
with separate local fixture homes. All 18 vanilla comparisons and the
other three cross-login trials also passed on the corrected CLI and host.
The before result, four-trial after receipt and vanilla log have SHA-256
`0a8ce0c42d7229d81d19fbe8133eb7c0bf8dc208ff8d06356ae474c91e8352b9`,
`f52e935a176a96c09a3a20ba5f42150924e699051636ab70359faa758d0d1ffb`
and `598639ea05f6f3ff319265cc02bfda8a96a7c412da8d2df9d20e1c8b9780d408`.
The three native pagination and recovery tests and the native Code Mode
transport test also passed with the matching host. Their receipt has SHA-256
`01ed9f1e9bdfdea61127835f7e41296607dc39969b03e3a6ddb7cd50ca068da7`.
[Corrected development source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-c9bb2bd018)

The normal package command for `86972ec0b5` reached Trust 15698. It
reported one unknown obligation and refused the CLI’s 66 in-process
procedural macro dependencies. It produced no package. The retained
compiler report has SHA-256
`196e2c03703554aa34df3e17899df26af789bdb54a13a3727a5e3ac6cb3b906f`.

Review `1065c63abf` integrates the connector, Code Mode and startup changes. Fork account
admission still runs inside the async startup body for all eight call sites.
All 32 existing ALab hook lines in that file were preserved. Upstream review
admission passed for all three commits from the prior review and all 425
from fork main’s integration point. The three-commit receipt has SHA-256
`3be4dab1b70bf23ba4f009fef1d26ec25befce3b235fafc40adc16dc56a4d8fb`.
The same source’s normal no-promotion package build stopped at Trust 15698’s
refusal of 66 in-process CLI macros. The report has one unknown obligation
and SHA-256 `ebcff52b15a681837f1c066041c4b3b58669f60e6b11fa66898f8369768150f5`.
[Integrated source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-1065c63abf)

The protected build produced CLI SHA-256
`6356748faf78bd9109d8f8304c6b47cf1ff39aae9ecc92eb1dcb138cc907321b` and
matching Code Mode host SHA-256
`a78fea357ebac656a692d0a478770a0d26a01f63fe0575b8a76f47f990d5745b`.
With Code Mode enabled, an ordinary `codex exec` returned `529.1681` on
its first tool call and `restored=1886` on the second, using the JavaScript
state established in the first. The CLI in a comparison layout without
the sibling helper reported that its host executable was missing. The
two-run receipt has SHA-256
`d379fcadd09a4f146621342b6e1d6f4e7c373e8adffc0eb9909ef38094d9d24f`.

The native gRPC/stdio integration test also passed with the retained host.
An initial Cargo invocation launched a different copy of the host from its
target directory. Corrected collector `7814a51c49` selects the compiled
test executable from Cargo’s artifact messages and gives it the retained
host at process launch. macOS reported the expected parent and executable
path. The named test passed, an absent selector failed, and the complete
receipt has SHA-256
`1862ab927dcd9b87da0b1b196b24c10d746201d4b7024955137d256effedd591`.
[Protected transport replay commands](../design/upstream-alignment.md)

The new connector WebSocket test delivered the authorized result to a
loopback peer. It checked for both plaintext and hex representations in
the sender's diagnostic logs. Neither appeared. The native run passed its
one named test, and the missing-name control returned 1. Receipt SHA-256
`1cb0ed4a3aaf9364fe077b2e2dba223a198641903549751dde7689e1a5d9ecec`.
The committed opt-in collector reproduced both outcomes, receipt SHA-256
`76e4d1d6724e48be2de7851d7512ea72082af77e8a615bea960a9dbf9bc08cb7`.
[Upstream WebSocket test](https://github.com/openai/codex/blob/6271f35f527bf7709b9af5e4e752e9f122706a17/codex-rs/codex-api/tests/responses_websocket_logging.rs)

On that matched pair, all **18 vanilla compatibility checks** passed
against official 0.170.0. The comparison measured 77 help pages
and 26,138 shared protocol entries. Its full log has SHA-256
`a9ccc8ccbb3a768c9550c3b208a449308d885a45432a5f8aa2f1e3fb2120e10e`.
All three bare cross-login trials passed again, preserving the original
thread, directory, tool history and B’s credentials, including while A’s
daemon remained connected.
[Exact migrated-pair receipts](managed-daemon-account-resume-2026-11-19.md#cross-login-resume-after-the-grpc-host-migration)

An additional upstream test exercises a different transition. A custom
provider disconnects during a turn with outstanding private connector
results, and the credential owner changes before the next retry. On
`1065c63abf`, the peer observed five requests on its first connection and
one on its second. The test expected one more request on a third
connection. It exited 101 at the connection-count assertion, before checking
the final request’s account or content. The raw test log has SHA-256
`98512282623268693a7d8ffe861a3bad9848d8e4e998c503aa270727f28777cf`.
An unchanged rerun captured the terminal event after the two old-owner
connections. It reported “application network permission was revoked.”
The sanitized event capture has SHA-256
`e5ea36f8b31d6ecb44889f1d9d80e0fe105ff71492372ece5b0b2688bb750b73`.
[Exact upstream test](https://github.com/openai/codex/blob/6271f35f527bf7709b9af5e4e752e9f122706a17/codex-rs/core/tests/suite/code_mode.rs#L2380)

Pristine OpenAI `6271f35f52` passed the unchanged test twice under the same
Trust toolchain. In both runs it opened the third connection and passed
the checks for the destination account and omission of private tool data.
ALab stops the running turn after its original network permission is
revoked. The fixture changes the external credential supplier on A’s
existing manager. The ordinary ALab account switch starts a fresh client
under B, which is also the path exercised by the passing `codex resume`
trials. Keep the old socket’s revocation and track automatic continuation of
this in-flight custom-provider retry as a development compatibility gap.
The two-source receipt has SHA-256
`b62c98e7351540c83087d67957620c681c3336bcfcef94fd3692e97c65ed7f20`.
[Account switching decision](../design/accounts-assignment.md#decision)

Two positive controls on that exact ALab source covered the neighboring
cases. A normal custom-provider turn retained authorized results within
the request budget (receipt `ae67d5afce95f598a8c375b87ee04cc0f98fcf5868089815898cf26bf4050850`).
Revoking a split WebSocket woke its reader even when the writer observed
the revocation first (receipt `5f05241cf947ab3bd38bb3a73fd8162f00500101eba5166a68907e178a5c7931`).

The core library test build on `1065c63abf` exposed a missing `CoreToolRuntime`
import in ALab’s model-retirement test. Review `3e47519928` adds that one
test import. Both upstream private-result ownership tests passed under the
count and byte limits, and the ALab test for a yielded terminal surviving
a retry passed on the same run. Its three-test receipt has SHA-256
`436a936da1fb8e1ddbd65f1cc418a54d6f35a7e6208a1ec659c6f0a534da326a`.
[Test-only development revision](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-3e47519928)

The normal source-selected packaging command also ran against `56bc1f03a3`
with promotion disabled. Trust 15698 refused to load 66 in-process
procedural macro dependencies for the CLI. The compiler reported one
unknown obligation, the package command exited 1 and the output directory
contained no package. The retained compiler report has SHA-256
`cd1a4421c8a6abe896a4c79c93193c6c8094e39ff0832121255b459e069f10b0`.

### Model promotion on the 0.12 release line

The 0.12 development build `61bc78c131` also resumes conversations created by
signed 0.11.0 and follows the provider’s model family. The preceding 0.12
build `d588c33d26` failed when the old model refused a request before output
and when it stopped after partial output. The backport passed both cases
over HTTP streaming and WebSocket. A pre-output refusal completed the
original turn on Sol 6.2. A partial reply appeared once, and the next turn
used Sol 6.2. A user-pinned model stayed pinned.

All 19 model controls passed on that executable with a loopback provider,
including continued use of the original shell for Sol, Astra and provider-defined
IDs. Interactive and noninteractive resume also passed. The macOS ARM64 CLI’s
SHA-256 is `505253499b14d3ffd08404556b831adf1a2b157d9209a6509f389c5cedcc7884`.
The signed macOS release remains 0.11.0.
[Exact 0.12 source](https://github.com/alabsystems/codex-alab/tree/alab/model-promotion-0.12-61bc78c131)

Private main incorporated the 0.12 recovery at `e1570ec546`. The source-selected
Core test binary passed all 11 recovery tests. They cover account ownership,
completed tool results, running shells and when replay must stop. On the
merged private tree, the feature tooling ran 207 tests with one skip.
[Private integration](https://github.com/alabsystems/codex-alab/commit/e1570ec546cbf8bc5b49e246442089d34bfb824a)

On the 0.13 line, selecting an attached side chat overflowed the default TUI
stack. Fix `c874746c62` passed both a fresh terminal trial and a resume from
signed 0.11.0. Both sent the final draft on its original thread and exited
cleanly. All 19 direct model controls passed on the same CLI.
[Side-chat repair](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-c874746c62)

Candidate `ddda3567c5` combines that repair, the private release worker and
all 426 reviewed upstream changes through `d52a39b307`. It keeps the 0.13
stream observer on the provider response before interceptors run. Applying
the 0.12 backport literally would add its older observer at a second point.
The candidate's native build and runtime checks are in progress.
[Reconciled 0.13 source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-ddda3567c5)

The newer 0.13 integration at `3431c96dfd` has committed intake decisions
for 425 upstream commits through `6271f35f52`. The last three add
account-checked connector results over WebSockets, gRPC for the local Code Mode host
and a shared thread-start future. Runtime qualification is in progress.
On the earlier `56bc1f03a3` source, the protected model campaign ran all
19 model tests. One method failed in all four of its shell cases because
macOS rejected a second Seatbelt sandbox before the shell could start.
The direct test above exercised the actual workspace sandbox and live shell.
[Exact 0.13 integration](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-3431c96dfd)

### Resuming the previous login’s work

The final default-all picker review `1b1eb96b99` passed five bare resume
trials and all 18 comparisons against vanilla 0.170.0. The ordinary
terminal/exec tests and the shared-daemon tests each saw A and B before
any input. The provider saw B on the selected A thread. The fork-main
integration `93ddb79b57` independently passed signed exec-to-interactive
resume with named B and with a long-lived A daemon after a shared-file
switch to B. The SHA-256 prefixes of those two fork-main receipts are
`f1e78bce76f6` and `f108a0cb9d1b`. These targeted checks belong to
their own source revisions. The 62 scenarios verified on `a98666774c` belong to
that earlier review. The reconciled review has its own 70 passing scenarios.
[Daemon and reconnect results](managed-daemon-account-resume-2026-11-19.md#reconnect-verification)

With the same local `CODEX_HOME`, sign in as B and run `codex resume` in
the enhanced ALab CLI. Choose A’s conversation from the initial list.
Terminal and exec work from every local login and project is shown in
recent activity order. On signed ALab 0.11.0 and official OpenAI 0.170.0,
use `codex resume <thread-id>` for a thread created by another account.
If the optional shared background server is enabled, the ownership-corrected
build checks its login and workspace before reusing it. A server still
serving A keeps A’s existing work, and B continues locally. On the signed
0.11.0 build, `codex --no-daemon resume <thread-id>` explicitly uses a fresh
local app server and B’s credentials.
Choose **Use session directory** when prompted to work in the original project.
For a noninteractive continuation launched elsewhere, use
`codex exec -C /path/to/original/repo resume <thread-id> "Continue the task"`.
[Resume controls and smoke commands](../README.md#resuming-local-work-after-switching-accounts)

The initial comparison used OpenAI tag `rust-v0.170.0` at `29eb79e986`, ALab main at
`0caa485b7e`, and the installed macOS ARM64 ALab 0.11.0 build `2026111701`
from `70d6391259`. Both ALab sources include upstream through `ce3031018e`.
Of the commits in the stable tag, five additional commits lead to its branch
point and four follow it on the release branch. The 399 pending commits at
OpenAI main `8e9746b19d` measure a separate development backlog.
[Official release](https://github.com/openai/codex/releases/tag/rust-v0.170.0)

At 01:10 UTC on November 19, OpenAI's latest stable release was still
0.170.0. Its development branch had reached `2d58165651` and its latest
prerelease was 0.172.0-alpha.1. ALab remote main at `29006ded2c` was 403
upstream-main commits behind. The 0.170 integration captured 118 of those,
leaving 285 development commits beyond its reviewed frontier. Of the four
stable-only commits outside the candidate's ancestry, one changes the release
version, one enables the Bedrock default already present on the integrated
mainline, and two have ALab backports for Mantle replay and the model catalogs.
[OpenAI prerelease](https://github.com/openai/codex/releases/tag/rust-v0.172.0-alpha.1)

A second fetch at 01:42 UTC found stable and prerelease unchanged. OpenAI
development main reached `d1d1ba0b83`. Remote ALab main at `1d948d68eb`
had 407 upstream development commits pending. The review branch had 289.

At 02:49 UTC the stable and prerelease versions and signed index 164 were
unchanged. OpenAI development had reached `508a429666`, remote ALab main
was `f182813f8f`, and their integrated frontier remained `ce3031018e`.
The resulting development backlog was 410 commits on ALab main and 292 on
the 0.170 review branch.

At 03:24 UTC on November 19, OpenAI development was `cf881a627d`.
Fork main `d97d6c7e98` had 411 development commits pending and the
published review source `aeb6e36ed7` had 293. Stable was still 0.170.0
and the highest published prerelease was 0.172.0-alpha.1. Signed index 164
still selected macOS ARM64 ALab 0.11.0 from `70d6391259` and Trust 27009.
[OpenAI stable release](https://github.com/openai/codex/releases/tag/rust-v0.170.0)
· [Signed package index](https://github.com/alabsystems/aterm/releases/tag/atpkg-index-164)

The 04:08 UTC pull found OpenAI development at `b5d9f7116e`, 415 commits
beyond ALab main `c7842167d1` and 297 beyond `aeb6e36ed7`. OpenAI's
latest stable release API still returned 0.170.0.

## What the integration delivers

On reconciled source `bbd706f710`, the fork owns 2,965 added files
and changes 701 upstream files. Another 477 upstream files are explicitly
listed for removal. The workspace lockfile is regenerated at sync. The
layer includes product code, tests and release tooling. Reproduce these
counts with `alab/delta.sh bbd706f710` against the integrated upstream
frontier `27e2227b2b`. [Layer measurement](../delta.sh)

| Surface | Comparison with vanilla 0.170.0 |
| --- | --- |
| Local session resume | Signed ALab 0.11 and official OpenAI 0.170 resume by thread ID after a login switch. The enhanced ALab picker also exposes threads from other local logins. The next turn uses the destination login. |
| Guardian and sidebar | The integration reuses private review history on the same live parent and resets it on process restart. A focused sidebar keeps its panel and draft. ALab also shows the focused account's quota. |
| Realtime voice | After replacing A's login in a running CLI, a fresh call used B but the observer used A and the test provider refused it. Resuming the original thread in a fresh CLI under B made both requests use B. Cancellation suppressed the pending observer. |
| Code Mode and DNS | Plain-text and HTML 503 admission responses received bounded retries. The locked dependency graphs matched the release with Hickory DNSSEC disabled. |
| Bedrock and model catalog | Built-in Bedrock Mantle and Runtime enable incremental tools by default. The integration normalizes Mantle commentary on replay and includes GPT-6.2 Sol and Astra provider entries. Provider fixtures cover the HTTP requests and responses. |
| Fork features | ALab keeps account assignment and quota recovery, aterm and Autopilot integration, persisted goal and process recovery, model-family migration, and its terminal controls. CLI and schema deltas are attributed to the fork or to later integrated upstream code. |

The repeatable CLI, Guardian, voice and image controls passed again on
`fcb2205723`. The private review branch preserves this exact source.
[Alignment candidate](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-fcb2205723)
[Cross-account results](#cross-account-terminal-results) · [CLI and protocol measurements](#measuring-public-cli-and-protocol-differences) · [Cloud release evidence](upstream-stable-0.170.0-2026-11-18.md)

## Initial signed-package comparison

On November 18 the installed ALab and OpenAI packages passed
`alab/smoke/dropin.py` against each other on this Mac. It compared 76 CLI
help pages, exec JSON events, local history file layouts, app-server
initialization, MCP listing, file-based login, and config overrides. The schema
comparison matched 23,725 shared definitions, properties and enum values and
attributed all 126 differences. ALab's fresh thread returned its configured
Ultra reasoning effort. Feature behavior needs its own checks, especially
across credential changes, retries and voice transport.

ALab already contains upstream's oversized-WebSocket HTTP fallback,
OpenTelemetry header refresh, experimental environment-scoped MCP names,
MCP OAuth race fixes, Cloud reconnect fixes, and permission and search fixes
in its integrated ancestry. ALab adds account assignment and renewal, provider
recovery, aterm lifecycle and Autopilot, saved goal and process recovery,
model-family migration, and terminal features including prompt stashes and
task controls. Each has its own acceptance scenarios.
[ALab feature inventory](../features.toml)

## Source gaps and acceptance checks

| OpenAI stable change | ALab main at the comparison commit | Check on the integration candidate |
| --- | --- | --- |
| Code Mode admission behind a proxy | The 0.170 fix for plain-text or HTML HTTP 503 responses is pending. | Retry transient admission failures within the vendor limit. Return permanent refusals immediately. |
| Hickory DNSSEC advisories | The audit configurations predate the five upstream exceptions. | Match both audit configurations and verify that the dependency feature graph still disables DNSSEC. |
| Direct Guardian reviews | ALab uses an isolated reviewer for each direct call. | Reuse reviewer history for repeated direct reviews of the same parent. Keep independent parents and credential owners separate. |
| Sidebar focus | Focusing the sidebar selects the chooser. | Retain the visible panel, draft and ALab's account row across focus changes. |
| Realtime voice | ALab has WebRTC audio and sends an SDP offer to the app server. | Exercise 0.170's TUI-created call and app-server attachment by call ID, including cancellation and replacement of the local login. |
| Bedrock incremental tools | The feature defaults off on main. The reviewed merged candidate matched the vendor with the option omitted, enabled and disabled. | Run all three settings over Mantle and Runtime. Inspect the actual tool declarations over the complete turn. |
| Mantle commentary replay and 6.2 catalogs | Main has the Mantle normalization and the Sol/Astra 6.2 catalog backports for first-party, Bedrock and Vertex routes. Signed 0.11 predates the cloud backports. | Check output order and citations on continuation, provider-qualified model IDs, explicit pins and existing 0.11 threads. |
| Release version | ALab has its own product version and upstream release ledger. | Verify the ledger entry and installed build manifest against the qualified source. |

The existing intake names the tests and affected ALab contracts. The release
review records the official ARM64 binary comparison for Bedrock and Vertex.
[Five mainline decisions](../upstream/1111483b48-from-1102123b27-intake.json) · [Cloud release evidence](upstream-stable-0.170.0-2026-11-18.md)

## Landing sequence

1. Stage the upstream-main prefix through `27e2227b2b` with the repository
   sync workflow. This is the first mainline point containing the five branch
   point changes and the Bedrock default. It brings 118 upstream commits from
   this base. The two later stable behaviors already have ALab backports.
   Reuse the existing intake decisions and remove duplicate patches when
   their upstream equivalents arrive.
2. Compare the resulting binary with the exact vendor release. Repeat the
   drop-in comparison and the feature checks above. Exercise local terminal
   resume from a signed 0.11 thread after switching logins. Record the thread
   ID, ordered history, destination workspace and credential used for the next
   turn. Include both explicit-ID and picker selection.
3. Land the candidate after the source-selected Trust gate passes, including
   dependency certification, ALab lint and tests, and the hooked upstream
   tests. Record the exact source and gate result. The last documented
   shipping attempt stopped before compiler verification on procedural macros
   with Trust build 15698. Compiler adoption is a separate dependency.
4. Record the 0.170.0 baseline for the next available ALab release,
   package and sign it through the designated publisher, then install from
   the managed channel on a test Mac and repeat the resume and drop-in checks.
   Record the archive identity and source reported by the installed binary.

[Sync and release process](../README.md) · [Trust and publication status](terminal-model-resume-2026-11-17.md#release-status-on-november-18-utc)

## Cross-account terminal results

### Bare resume across local logins and projects

Fork commit `cccbf2e1d7` makes the enhanced startup picker open one
list ordered by most recent activity. An account switch or a change of
project no longer asks the user to remember a flag. Recent-first ordering
also keeps yesterday’s unfinished work easy to find after switching back.
The existing archive control includes archived conversations on request.

On the 0.170 review `2d572ce397`, two tests typed literal `codex resume`
in B’s project. B’s newer conversation and A’s older conversation appeared
together before any input. Selecting A continued the same UUID, retained
A’s command output and executed a new shell command in A’s original
directory using B’s login. A’s original command ran once. The same check
passed with A's conversation created by signed 0.11.0 interactively and
through `codex exec`. A second invocation independently narrowed to the
current directory and to the current login. Both left only B’s conversation
visible. The synthetic accounts shared one isolated local Codex home.

| Check on review `2d572ce397` | Result | SHA-256 prefix |
| --- | --- | --- |
| CLI executable selected by Cargo | Frozen test binary | `7df2978389d2` |
| Vanilla 0.170 CLI and protocol comparison | All 18 passed | `f5fd3174aad2` |
| Native resume picker suite | All 135 passed | `8da230b2de21` |
| Signed terminal conversation resumed under B | Passed | `338aa273b7b5` |
| Signed exec conversation resumed under B with two named login files | Passed | `2e20bc3fd789` |

Fixture revision `b9866da51b` reconstructs complete terminal screens from
the cursor positions and erase operations captured by the real PTY. Its
16 account-resume unit tests passed. The signed ALab 0.11.0, official
0.170.0 and earlier opt-in ALab picker controls each failed the opening
cross-login list check as expected.
[Picker implementation](../../codex-rs/alab-tui/glue/resume_logins.rs) · [Terminal check](../smoke/account_resume.py)

The help-bearing review `bf31719ce3` repeats both bare continuations,
all 135 native picker tests, four execution-wire tests and all 18 vendor
comparisons. The signed-to-review terminal and exec result SHA-256 prefixes
are `bf8dfdb79e98` and `c1a0366992ab`. The new executable has SHA-256
`b3fa8aec8bf58293baee6a5b1636725db8ae4441e93ed2da87e466959f90f32e`.
Its actual `resume --help` explains the local picker and distinguishes
`--last`, which uses the current login. Its vendor report
(`0d1dfdf46763`) compared 77 help pages and 26,145 shared protocol items.
The ten vendor-only schema entries belong to its single experimental
`currentTime/read` callback. The review incorporates OpenAI’s subsequent
move to synchronous host-provided clocks.
[Exact picker review](https://github.com/alabsystems/codex-alab/tree/alab/resume-default-0.170-bf31719ce3) · [OpenAI clock change](https://github.com/openai/codex/commit/4aac2eacc5)

The separate managed-daemon trial retained a live observer on a daemon
started as A. After the shared login file switched to B, B’s independent
exec used B. The resumed turn on the old daemon and its tool follow-up
both used A’s bearer and workspace. Current-login narrowing also showed
A’s thread. The observer saw the original UUID load on that same daemon
and confirmed normal shutdown. This trial exposed an ownership defect in
the early default-all picker. Result SHA-256 on `2d572ce397` is
`2aab227182b4c2250c09b665ebfac503d9d5b4820dcb7525a459bc0747049780`.

The fix on 0.170 review `14be20c99e` passed all three managed-daemon
cases with distinct users and workspaces sharing a display email. All five
bare resume trials used B’s credentials, including the two ordinary terminal
and exec cases. The 137 native picker and login tests passed, as did all
18 comparisons with vanilla. A direct build of fork main integration
`33afb26768` also passed its normal signed exec-to-bare-resume test with
both named credential files unchanged. The respective executable SHA-256
prefixes were `976109f01b75` and `c7faa76da505`.
[Daemon results and correction](managed-daemon-account-resume-2026-11-19.md#verified-correction)





In the default-all ALab CLI, switch to B, run `codex resume` and select
A’s conversation. Both logins must share the same local `CODEX_HOME`.
Signed ALab 0.11.0 uses the thread ID shown in A’s exit summary. The
earlier enhanced review `a98666774c` required selecting **All local**.
It also required `--all` for other directories and
`--include-non-interactive` for exec-originated conversations.
[ALab picker instructions](../README.md#resuming-local-work-after-switching-accounts) · [OpenAI CLI reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli#codex-resume)

When the current login has no matching sessions, the enhanced picker shows the
keys for switching to All local. The hint in `ff08aea7c7` was rendered in a
16-column terminal and tested with remapped and unbound controls. All three
protected TUI tests passed. The test log has SHA-256
`e64f7e352b705c2e50568974e06450c5e6fd1ab01211bf0315ae506118465c97`.

Two synthetic ChatGPT logins shared one scratch Codex home. Account A ran a
workspace command that recorded a unique marker. A fresh CLI under account B
continued the same thread. The loopback provider checked B's bearer and
workspace header, the original command and output, the previous dialogue,
and a distinct completed turn. The workspace recorded one execution.

On November 19 UTC, the cross-project terminal check started B in a second
directory and opened `codex resume --all`. With Current login selected, A's
row stayed hidden. Selecting All local exposed it. Switching the directory
filter back to Cwd hid it again, even with All local selected. Reopening All
and choosing **Use session directory** continued the original UUID under B.
Both completed turns recorded A's original working directory and the command
ran once in A's workspace. This checks the two independent filters and the
directory recorded after selection. B's continuation in this run did not
execute a second shell command.
The seed was signed ALab 0.11.0, the resume binary was `aeb6e36ed7`, and
the result JSON SHA-256 was
`e7570ad40fcae9b88138f7f3c24f8e10366825187b3eaeea622e457e710a8ce8`.
The ID and same-directory picker controls passed on those executables again.

The loopback gateway also checked credential ownership on each model catalog
and account-information read. A successful turn alone could miss a read sent
with the earlier login. Signed ALab 0.11.0 and OpenAI 0.170.0 both passed this
check by ID. The signed-to-`aeb6e36ed7` ID, same-directory picker and
cross-project picker all passed. Each phase issued both kinds of read.
Every observed seed read used A's bearer and workspace, and every resume read
used B's. Adding one stale bearer label to a copy of a completed destination
catalog observation made the verifier fail even though the turn succeeded.
The cross-project result SHA-256 was
`7d88dbd6fd30f7e61b74a5e4e4414cf9149d538151362ebf551615ee4ff96a9c`.
These observations cover fresh CLI processes. The retained-worker cases in
the account-isolation contract still need separate execution.

A second account layout kept both synthetic logins active simultaneously in
separate `CODEX_AUTH_PATH` files and shared one conversation home. Signed
ALab 0.11.0 resumed by ID under the destination file. All three
signed-to-`aeb6e36ed7` routes passed, including the cross-project picker.
The original thread retained both completed turns. All post-switch model,
catalog and account reads used B, both credential files kept their pre-run
hashes and the shared home had no default credential file. This exercises
ALab's parallel-login arrangement as well as replacing the shared login.
Signed ID result SHA-256
`d95f5f114388b1ca71d33242d836ab74b62b4e6c726762a72e925feca965fe3c`.
Cross-project result SHA-256
`1b40dafd5b00ca37add515c0329fdc4d5ab3a5c12ad09ca5776d14f18f1b85af`.

The final cross-project check gave each project a different marker. B then
ran a distinct shell command in the resumed turn. Its `pwd -P` and marker
output identified the original project. Each command ran exactly once there,
and both counters remained absent in the second project. The model requests
used A, A, B, B on the same original thread. This passed with the shared
login file and with two already active named files on signed 0.11.0 → candidate
`aeb6e36ed7`.

The gateway now joins every accepted HTTP handler before checking the final
observations. Closing the listening socket alone previously allowed a passing
report with a late source-account read during destination resume. The revised
fixture rejected that same observation. The combined check passed on both
cross-project layouts, the named same-directory picker, signed named ID and
vanilla 0.170 ID. The stronger test distinguishes a successful conversation
from a successful command in the selected directory and from a complete
credential-read observation.

| Combined receipt | SHA-256 of result JSON |
| --- | --- |
| Signed → candidate cross-project, shared login | `99de2533bc36195b18fe2a23b9eba69e984b4fe5512c92d9b5d65a17072ac813` |
| Signed → candidate cross-project, two active named logins | `6a29c38ba589a9a64fbf48cf34dd010ff85c7913d98b98e4a5c7e3613d7c0fef` |
| Accepted late wrong-account observation rejected | `24ff76130f85aaf59088ef3ea0819966b7339fd943bbd5542ebea90393d0085d` |

After integrating the destination-renewal fixture on `c7842167d1`, all
eight account-resume unit controls passed. The shared and named cross-project
routes and named ID also passed again. In the expired-destination cross-project
case, B renewed exactly once, persisted the new credential and used it to finish
the original thread in A's project. A's credential stayed unchanged. The seed
was signed 0.11.0 and the resume CLI was `aeb6e36ed7`. Result JSON SHA-256
`d7c31002ed160e3dc2b3dd51e2f39d53828557df6c0d715dc780b3fdbc8417af`.

#### Interactive to noninteractive continuation

A new acceptance surface starts a real terminal session under A, exits it,
and starts `codex exec --json resume <thread-id> "Recall our cedar marker and continue."`
under B. It checks the guardian's command and successful exit, the exact
thread ID in JSONL, one assistant reply between turn start and completion,
and the completed turn in the original rollout. A regression control rejects
a reply after completion, another thread's ID and a failed turn. B's provider
request must include the original completed tool result. The workspace
counter must still contain exactly A's single execution.

Five runs passed on this Mac using synthetic accounts. Vanilla 0.170.0
and signed ALab 0.11.0 each resumed their own terminal by ID. Signed ALab
also seeded the reconciled candidate in three cases covering a shared login
file, two active named files, and a named destination that had to renew
exactly once. The three model requests in every run used A, A and B.
The signed named result has SHA-256
`f6f846f2d8bc3707ff6a91860ec55d3fa0a676ae7d8f18cc08be0f42e263cd93`,
the vanilla result
`919ab6118202178100bc9413d091e29ebf8eb390ab35c2fd54017ed5b04df687`,
and the signed-to-candidate renewal result
`87754ed2d90684efbc62a5ead5ec39147a3044ee748e0a0c97228e0bb16c880f`.

| CLI used to create the thread | CLI used to resume | Result |
| --- | --- | --- |
| Signed ALab 0.11.0 | Signed ALab 0.11.0 | Explicit ID passed. The default picker showed an empty scoped list. |
| OpenAI 0.170.0 | OpenAI 0.170.0 | Explicit ID passed. |
| ALab source `496a8024ab` | Same source | Explicit ID and bare picker passed. The picker showed the thread after selecting **Logins → All local**. |
| Signed ALab 0.11.0 | ALab source `496a8024ab` | The bare picker resumed the original signed-build thread under B with the same history and tool output. |

The picker runs used the normal daemon selection. All 132 resume-picker Rust
tests passed. The macOS ARM64 CLI test executable had SHA-256
`c794838d7579915a142d75ada3336d002d0b341169fa6982ecfa74c6115d227a`.
A separate `alab/package.sh --no-promote` attempt on the same source stopped
when Trust 15698 refused the CLI's in-process procedural-macro dependencies
before verification. The installed package on this Mac remains 0.11.0.
[Repeat the terminal tests](../smoke/account_resume.py) · [Picker usage](../README.md#resuming-local-work-after-switching-accounts)

## Integration and qualification

The repository sync merged the 118 reviewed OpenAI commits through
`27e2227b2b` into candidate `a4e1851fb6`. The merge required 34 conflict
resolutions. Its clean gate run accepted every committed intake decision and
the ALab hooks. It then requested execution evidence for 14 affected feature
contracts and stopped before compiler certification. The gate log has SHA-256
`33d0f38963008a98ad8255602b610d48ae86b86b5504a35c4bc465b58baae9ee`.
The retained sync worktree and its committed resolution notes can be inspected
with `alab/sync.sh --status` and `git show a4e1851fb6:alab/upstream/27e2227b2b-resolution.md`.

The protocol generators produced both compressed schema exports, including
`alab/retainedConfigured` in the stable and experimental surfaces. The
protocol crate passed 333 tests, with one ignored generator test exercised
separately. The Python SDK passed all ten tests.

The candidate carries all five Hickory advisory entries from the release
in both audit configurations. All-features Cargo resolution, including Linux
targets, enabled only `futures-io,std,tokio` on `hickory-proto` 0.25.2,
`system-config,tokio` on `hickory-resolver` 0.25.2 and `default,std` on
`hickory-proto` 0.26.3. Every Hickory DNSSEC feature was disabled. OpenAI
commit `7194b21776` moved the Linux sandbox's 0.26.3 dependency from test to
runtime. The tag tip changes only the workspace version and leaves the member
versions in its lockfile at 0.0.0. The release-side locked resolution used
the immediately preceding commit `176cbbe92c`, with that same lockfile.
An offline resolve of the tag tip in scratch updated only the versions of
its 174 workspace members and confirmed the same Hickory features.

The two-login terminal control now exercises both directions of the picker
filter. Narrowing to **Current** removes the earlier login's row, and expanding
to **All local** restores it. This passed with both a signed 0.11 seed and a
development seed. Removing the narrowing keystroke made the control fail
before the destination submitted a turn. The regression landed on main as
`2433ad92ca`.

OpenAI 0.170.0 was also exercised with bare `codex resume` in the isolated
embedded configuration. The picker reported zero sessions under B. The
original A session and tool output were present on disk, and the provider saw
no resumed turn. Explicit ID passed on that same vendor executable, as shown
in the table above.

The first combined compiler check caught changed inputs in the Core steering
fixtures, MCP status, and the retained TUI. Candidate `5b6e60c86a` incorporates
the resulting adaptations. In particular, an unfinished plan retains its
original item ID across native transfer, and live new-conversation drafts
still serialize with the version-one action name. Both have focused regression
tests. The native Core transfer regression passed. Six focused TUI tests
also passed on `5b6e60c86a`, covering the version-one action, sidebar parent
draft, retained session metadata, keyboard selection, grid hit testing and
realtime call cancellation. The voice test verified that restarting attached
only the new call. The account/quota row during a sidebar focus change and realtime
call attachment during login replacement were still pending on that source.
Three public app-server integration tests passed on the same candidate.
Guardian reused private review history for the same parent and isolated a
second parent. Both Bedrock Mantle and Runtime advertised the expected
optional ultrafast tier while preserving their default service tiers. These
runtime checks used fixture providers. Live AWS entitlements are unmeasured.

The skill-discovery recipe landed on remote main in `53266fc553`. A bounded
Rust filesystem test covers the missing-root watch. A real app-server test
covers root creation, skill edits, vendor reinstall and cache invalidation.
The latter passed on signed 0.11 and through the protected recipe on source
`a1663da557` (84.502 seconds, command exit 0, result SHA-256
`4c4b8de6d5bedae455f97d9b2c8996d514beec99b47e7d814b4cb20e424b6e4b`).
The managed Python path on this Mac contains spaces. The native-gate test
fixture now quotes the interpreter path, and all 16 campaign tests passed.
The concurrent change allowing RPC callers to specify deadlines is included
in the merge. All 11 account-quota fixture tests passed.

Seven affected contracts lacked complete execution recipes at that checkpoint.
They covered 53 declared scenarios across account isolation,
live exec handoff, execution continuity, goal storage recovery, native session,
native UI and qualification fixtures. Seven scenario descriptions explicitly
record outstanding native lifecycle requirements in account isolation and exec
handoff. The next qualification work is to exercise the affected scenarios
with their real lifecycle owners and collect the exact-source evidence bundle.
[Scenario inventory](../features.toml) · [Execution recipes](../feature_recipes/scenarios.json)

The locked source check for `codex-cli`, `codex-core`, `codex-app-server` and
`codex-tui`, including test targets, completed on `5b6e60c86a` in 4m38s.
It emitted warnings and made no compiler-proof claim (log SHA-256
`269d17054e644820207836f9c7df3696c78cdf5c50ec8e680004d6f55915f29b`).
The upstream Code Mode admission test passed on integration source
`a4e1851fb6`. It exercised plain-text and HTML 503s, the retry limit, and
immediate permanent refusal (log SHA-256
`51c8c592886f4b26ae62af9cd3945c027321a1c0ddea28cc2afcbc8e11e47cc6`).

After incorporating remote main `53266fc553`, the clean integration candidate
was `21e660cf42`. The private review branch
`alab/alignment-0.170-21e660cf42` preserves the whole merge and the checked
source for other clones. The locked four-crate compiler check, including
test targets, passed again on this exact commit (1m52s, log SHA-256
`bb4526889ede7631c5ab953a30f6893be8f5d24fc21e89bd62a90dc8218a6341`).
At 00:16 UTC on November 19, the latest fetched stable was 0.170.0 and
OpenAI main was `f7dece07d0`. ALab main had 401 pending upstream commits.
The review branch had 283 pending after integrating the selected 118.
Its native gate admitted the same 118 intake decisions and
stopped on the seven feature scopes above (gate log SHA-256
`c87ba8c004e1e68b632cb2e7d96fe1f0332133d54da1a2a47b22584998e9bc29`,
sync exit 4). Separately, `alab/package.sh --any-commit --no-promote` on
`a4e1851fb6` reached the dependency-inclusive certified build. Trust 15698
refused the CLI's in-process procedural macros before function coverage and
reported an inconclusive verdict. Packaging exited 1 (log SHA-256
`0f02fdb507fae48a7a1566894e29a389809e48f72b0a5785b61dc88da879ca10`).
Resume the integration with `alab/sync.sh --continue` when its feature bundle
can run. Shipping also needs a source-selected compiler that certifies the
CLI and dependencies, then the release ledger and signed publisher steps above.

Candidate `5b6e60c86a` passed both signed-0.11-to-candidate terminal resume
checks. On each run B sent the third provider request on A's original thread,
with B's bearer and workspace, the original command and its output, and a
fresh completed turn. The bare-picker run also checked both directions of the
login filter. The candidate test executable has SHA-256
`2bffcadc525b19e9effbe79be987c7c1e484bee25137661175e156c43047dd2e`.

The Guardian comparison exercised 20 direct reviews across that executable
and the official 0.170.0 CLI. A second review on the same live parent received
the prior private rationale. On both clients, a fresh process resuming the
same parent under B used B's bearer and workspace and received no rationale
from A. Restart under A gave the same reset, and a separate parent started
with empty private review history. The recorded tool calls remained unexecuted.
A deliberately leaked rationale was detected in the resumed request. These
checks cover process restart across logins. [Repeat the Guardian comparison](../smoke/guardian_account_resume.py)

### Measuring public CLI and protocol differences

The first run on the merged CLI exposed two attribution errors. The fork's
`debug aterm-model-policy` command has an inline platform attribute. Also,
the generic property name `threadId` survives on unrelated upstream types
after upstream commit `4aac2eacc5` removed the experimental server-to-client
`currentTime/read` request. The corrected comparison binds each property and
enum value to its export file and owning type, checks the binary exports
against the committed sources, and recognizes the annotated fork command.
For example, `RetainedFrontierNotification.threadId` belongs to ALab, whereas
removal of `CurrentTimeReadParams.threadId` comes from upstream.

| ALab executable against OpenAI 0.170.0 | CLI help pages | Help differences, ALab / upstream | Shared schema items | Schema differences, ALab / upstream |
| --- | ---: | ---: | ---: | ---: |
| Installed signed 0.11.0 | 76 | 1 / 0 | 26,155 | 141 / 0 |
| Integration source `5b6e60c86a` | 77 | 2 / 4 | 26,145 | 174 / 70 |
| Review branch `21e660cf42` | 77 | 2 / 4 | 26,145 | 174 / 70 |
| Side-chat fix `5ca190d357` | 77 | 2 / 4 | 26,145 | 174 / 70 |

The counts include qualified names in the combined v2 schema bundle.
They supersede the earlier counts above, which used the original comparator.
Every row passed the companion checks for exec event order, local
history layout, app-server stdio, MCP listing, effective configuration and
file-based login. [Compatibility comparator](../smoke/dropin.py)

The frozen CLI built from `21e660cf42` has SHA-256
`df83b4ce3aae0437b5d3435f8c90140cc316e8309ce7f274bf9f189d1d33f55a`.
It repeated both signed-seed terminal resume checks, all ten Guardian reviews
across same-login and cross-login restarts, and the full drop-in comparison.
An independent control removed `threadId` from the real executable's
`v2/ThreadReadParams.json` export. The comparator reported that exact field and
exited 1. Its 26 unit tests passed.

Candidate `5ca190d357` independently repeated all 18 drop-in checks,
the two signed-seed resume routes and ten Guardian reviews. Its voice
credential race and inline/path-backed image replay controls also passed.
Remote branch `alab/alignment-0.170-5ca190d357` preserves this source.
Both resume
routes used account B on the next request, retained account A's completed
tool output and continued the original thread. Its test executable has
SHA-256 `762c5f3442bf6904872ced0879224f3da23ab1bb60a918c671a030900d4c3150`.

The native test executable for `96042e0c18` has SHA-256
`055dbdd11e8c0058d3a39d3e227dd4c31d2d8749a92b99e084183d8e762944b0`.
It repeated the 18 drop-in checks against the official 0.170 CLI, with 77 help
pages, six attributed CLI differences and 26,145 shared schema names. Both
signed-seed resume routes used source, source, destination credentials on the
same thread. The original command ran once. The Guardian, voice, cold and
signed-seeded sidebar, image replay and simultaneous goal-write checks passed.
The goal check observed six committed mutations and twelve wire messages
across the loaded and unloaded threads.

The repaired `fcb2205723` test executable has SHA-256
`6a2c0a5c365989efad4bb5733116141b764e9b69e8ead96b3fc28b5145649f01`.
Its 18 drop-in checks, two cross-account resume routes, ten Guardian reviews,
cold and signed-seeded sidebars, WebRTC and image replay controls passed.
Both resume routes observed two completed turns on one UUID and the request
sequence A, A, B. The simultaneous goal mutations and the separate client
disconnect control also passed. The disconnect receipt has SHA-256
`72834218865b6cb0b4272743ccb2e663163d14cf40b16472b37183be7a4c971d`.
On the first run three terminal controls stopped during initial executable
fingerprinting. The byte digest matched the frozen build and all three passed
when rerun with fresh fixture directories.

The ordinary cold `/side` command on the unoptimized 21e test executable
overflowed the 16 MiB main thread stack. The signed ALab 0.11 and official
0.170 binaries completed that same command. On 5ca the fork future is
allocated before the UI hands it to Tokio. The outgoing task shrank from
344,128 to 192 bytes. Cold creation completed with the default stack.
The PTY showed the same visible side chat, both drafts and the focused
account's quota after switching to the child and back. Resuming a signed
0.11 parent with an existing child passed on both 21e and 5ca.
[Side-chat decision and terminal evidence](sidebar-focus-account-row-2026-11-18.md)

A native check of the frozen 5ca branch against later main `e346b5fc51`
stopped at the hook comparison. Main counted 435 account hooks and 27 test
hooks, compared with 431 and 18 on the frozen candidate. The recorded gate
exited 1 with log SHA-256
`defd0a66250c74160f3050019acfacb00002eb36772711a643d897deecf426bd`.
The subsequent `96042e0c18` sync merged main through `953513eef6` and passed
that hook comparison.

### Voice and credential ownership on the original thread

The candidate and OpenAI 0.170.0 exercised the entire two-request voice path
with synthetic accounts A and B. Holding A's call-create reply while changing
the login file to B still allowed the running server to attach to A's call.
Cancelling a pending B call produced zero observer requests. Starting another
B call in that process sent B's credentials on creation and A's credentials
on attachment. The loopback service refused the mismatch and the terminal
displayed the error. Both clients recovered by running `codex resume <thread-id>`
under B. The original UUID stayed the same, B authenticated both requests,
and the terminal reported the voice start and requested stop.
[Repeatable voice comparison and its measurement boundaries](webrtc-account-resume-2026-11-18.md)

### Collecting evidence while the integration waits

The native gate request for `21e660cf42` covers 14 contracts against main
at `53266fc553`. Keep that candidate and reviewed base fixed while observing
those contracts. Moving either endpoint creates a new scope to qualify.
The collector in `9dde204206` runs one affected feature at a time and
records its native test logs, source, upstream and base. Its partial result
keeps every other scenario empty. A complete campaign can reuse the recorded
checks at these same endpoints. The campaign suite passed all 18 tests,
including replay of a valid partial result and rejection of incomplete
release evidence. The wider feature suite passed all 168 tests.
[Collector usage](../design/upstream-alignment.md#validation-and-reporting)

The native UI control in `e02f8fd20d` measured two distinct saved input
types. Two PNG path pastes reached the provider in order on one turn. Scrolling
the transcript and reopening that thread with the files present painted the
original red and blue thumbnails. After both files were removed, the same
message retained its two numbered labels and emitted zero Kitty graphics
transfers. A separate thread created through the public app-server with inline
PNG inputs restored both original colors after file deletion. OpenAI 0.170.0
returned the same path and inline input types from its public thread history.
Keep the original storage semantics. Duplicating every path-backed image into
public history would disclose bytes through a field that currently stores a
path. [Terminal and public-history control](../feature_recipes/native_ui_controls.py)

The first protected partial collection on `21e660cf42` completed skill
discovery. One Rust test verified the missing-root watch policy. A separate
app-server run observed the new root, actual edits and a vendor reinstall
in 84.377 seconds. The evidence contains all 14 reviewed features and 97
scenarios, with observations on the single skill scenario. A second collection
validated the captured logs and returned the identical part with SHA-256
`fe8b8d58b7166dbe27261b766102dfcb61d76e010dc1ef7ce0a013ef6c95a83b`.

The protected qualification-fixtures part on `fcb2205723` covers six
scenarios with 34 named observations. Core supplied two process-lifecycle
checks, boot supplied two, app-server supplied three helper-account checks,
the API suite supplied eight, watchers supplied two, the two TUI suites
supplied 16 and the native unit-comparison adapter supplied one. Every stage
exited 0. Independent replay of the seven receipts reproduced part SHA-256
`86efc89a8228fc088b380280dca22b771ee329436507b621fe33543b55226109`.
The combined campaign regression suite passed all 178 tests. An earlier
canceled TUI execution on `96042e0c18` changed its log after exit and was
excluded. The subsequent custodian keeps the workspace lock until the native
controller finishes. [Cancellation decision](feature-stage-cancellation-2026-11-18.md)

On the protected native-UI run, all 14 named Rust tests passed. The terminal
fixture then waited for a project named `work`, while Codex rendered
`Starting | codex-alab` and `Ready | codex-alab` under the repository's
evidence directory. Commit `b63881b16e` checks that the same project moves
from Starting to Ready. The corrected protected run exercised all four views
and both image turns on the same helper binary. Its native test result has
SHA-256 `c0b5e79623c66a11d1a701258795ad874ceee1e6b33f288af2c5e93e4e9d89a9`.

Three more protected fixture observations identified concrete collection fixes.
The pagination fixture named a retired test module. With the five current
ALab names and the three upstream names, all eight passed and the evidence
reader admitted the four original scenarios. The notice suite opened enough
concurrent SQLite pools to exhaust the process's 256 file descriptors. All 13
checks passed twice with the native gate's four test workers and the same
descriptor limit. The skill fixture now reads its cached description after
draining pending watcher notifications. A real trailing edit reproduced the
original failure, and both the standard and trailing-edit trials passed with
the corrected precondition. [Pagination evidence](history-pagination-selectors-2026-11-18.md)

The compaction integration binary later hit the same descriptor limit while
opening its thread writer locks. Apply the four-worker default across Rust
recipe stages and honor an explicit worker count. Under the original limit of
256 descriptors the exact `fcb2205723` binary passed all ten compaction tests
twice, peaking at 212 and 210 sampled descriptors. The two-worker control also
passed. The WebSocket suite passed 57 tests and the transport suites passed
406 before this resource failure.

### Protected evidence on the review branch

All nine parts identify source `fcb2205723`, reviewed base `1bf2f20e36` and
integrated upstream `27e2227b2b`. The original collector ran on the frozen
source. The corrected collector at `ad3891f770` executed the same source for
history pagination, native UI, security notices, skill discovery and provider
recovery. Its full regression suite passed all 178 tests. Each part retained
all 14 contracts and 97 scenarios in its inventory. The counts below cover
observations attached to the named contract.

| Contract | Scenarios observed | Named checks | SHA-256 of evidence part |
| --- | ---: | ---: | --- |
| build-and-adoption | 4 | 128 | `9d52790bf5efbbf42fbf845f818c7de5bf24ad30484cc28664a6a618f3681930` |
| history-pagination | 4 | 8 | `3a9e8a71450a876df148c66c039fec38b1ea27a625034b2200b29bca602b16b2` |
| model-discovery | 8 | 35 | `0237c7d0eb54854e2b9806903b6c12f68b18865df3ebd78e828be162a9eeda12` |
| native-ui | 5 | 15 | `16285caf762bc0e0f3d93b404139ee7faf6246c03bb10715472dfb075d75d660` |
| product-policy | 3 | 18 | `ee173d0737c534da501a6ee67ce40c5281516a2d84a4e7e113f06dd45469f427` |
| provider-recovery | 23 | 200 | `7ed43007405af7158d9adcc342b47786449e04ef3f2bbb7334d0b568790bf1d0` |
| qualification-fixtures | 6 | 34 | `86efc89a8228fc088b380280dca22b771ee329436507b621fe33543b55226109` |
| security-notices | 1 | 13 | `71a427cebb3fed062102ff6809c48c0eaf7c71bbc9191e40012869466e583305` |
| skill-discovery | 1 | 2 | `920a9536a065b420d23f2107baeed0a759070914c03215a4e15cc0ef62de991e` |

The evidence reader revalidated the underlying logs while combining all nine
parts. The full campaign imported them and exited 1 with the five unsupported
contracts listed below. No source qualified for release. The separate package
certification report has SHA-256
`25f63eace44001f53f7e35aaaa41eddd046069192bd406076f563c35d5c93181`.
[Collector usage](../design/upstream-alignment.md#validation-and-reporting)

### Reconciling subsequent ALab main changes

By 03:04 UTC on November 19, main `d1f61eef62` had acquired two defaults
hooks and 25 misalignment-review hooks beyond the frozen review source.
The comparison of `fcb2205723` with that main correctly stopped at hook
comparison (log SHA-256
`a693f4c60a15b890e39f75eca72e48e9de1c44ec53bf3b224e6e70b2c4a02092`).
The repository sync merged all concurrent main changes cleanly on
`aeb6e36ed7` while keeping upstream at `27e2227b2b`. Its gate admitted all
118 intakes, passed the hook comparison against `d1f61eef62`, and reached
the missing exact-source bundle (log SHA-256
`5c3e7cd7a916d01e21cb9f42705ae9e6305833507c2164ef2826994bd9b71dbf`).
The reconciled request contains the same 14 contracts and 97 scenarios.
The 55-scenario evidence set above identifies its original `fcb2205723`
source and remains available on that review branch.

The reconciled executable has SHA-256
`73c127a0d999b9fd7dee4a0d342f44956638b0193f48a00b35563426502bf8a4`.
Both signed-to-candidate resume routes used A, A, B on one original UUID and
completed two turns. The entire 18-check vendor comparison repeated with
77 help pages and 26,145 shared schema names. The Guardian, voice, cold and
signed-seeded sidebar, image replay, overlapping goal-write and disconnected
client controls also passed. Its own package attempt stopped on the same
procedural-macro certification refusal with zero claims proved and one
unknown. The package log has SHA-256
`73d4b45fcdf6012c934da4612e2fc112af66d0aca818d9f79ab8171e61f22091`.
The reconciled source collected its own history-pagination part. All eight
native tests covered its four scenarios, with evidence SHA-256
`abc65665218eb8adea2643122892c260178486dd38664be5efe846ff0bcd2bd0`.
[Reconciled alignment candidate](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-aeb6e36ed7)

#### Protected evidence on the reconciled source

By 04:07 UTC on November 19, all nine supported contracts had independently
executed on `aeb6e36ed7` against base `d1f61eef62`. The collector and this
candidate have identical feature recipes and evidence readers. Both the
candidate reader and main's updated reader revalidated the native logs,
combined the nine parts, and reported the same 55 of 97 scenarios. The
manifest SHA-256 was
`a0cc0db8b8dd5e03be611665318580ab12f3f76f9d8288ce4eb66d23457dff5f`.
The complete campaign still exited 1 with the five unsupported contracts
listed below. Their 42 scenarios require further work.

| Contract on `aeb6e36ed7` | Covered scenarios | Distinct named checks | Part SHA-256 |
| --- | ---: | ---: | --- |
| build-and-adoption | 4 | 128 | `b35ea37791c3b8a1db4941cefab36a5d0d01a3090099c26c4e5475e9df660db3` |
| history-pagination | 4 | 8 | `abc65665218eb8adea2643122892c260178486dd38664be5efe846ff0bcd2bd0` |
| model-discovery | 8 | 36 | `6c4f5c34505e7217baa128f989d7407993ce90827e1f513908ee3da6bc033a51` |
| native-ui | 5 | 15 | `7ed9cdcdacd8f08a8a9123f1b642312530d98717ba32953f5a756307e986bad0` |
| product-policy | 3 | 19 | `bf2621ccf26606b0470e84dfad5f0ed04d97d78642f5f0a2d23f741ebb8ea438` |
| provider-recovery | 23 | 200 | `9638bda08b08da54c6f10581a35abc8482a90182258bab924e6dfc986b6202cc` |
| qualification-fixtures | 6 | 34 | `e7f446addd7ba0f8ccc2831c0e16ad2a6914684434af713de654bde6b72a5714` |
| security-notices | 1 | 13 | `2d81ddf509b01b5dbead2702c5d2a4c07af0b249c207119adb9eaccb9ffbfb9f` |
| skill-discovery | 1 | 2 | `0328365840780076ace4097be6e62d50fa04d80454d7bf5e382a9ae28c29cce9` |

Provider recovery first reached the running disk floor during autopilot
integration. The gate stopped with status 75 at 19.64 GiB free. Retiring
the completed snapshot test's generated debug cache restored 34 GiB. A
fresh provider campaign then passed its 23 scenarios with 200 named checks.
The package certification decision remains recorded above.

The next protected run on `aeb6e36ed7` covered execution-continuity
scenarios 1, 2, 3, 4 and 6. Four CLI controls checked actual hook execution,
model migration, MCP form handling and manual title editing. Three native
CLI tests checked the parent, sample and fork histories, including cold
restart with legacy and paginated reads. The part SHA-256 was
`409c2af49eba51c73f30348dd79df96263fe0fbfb5c3dd244c78dca7760163d0`.
Revalidation with the original nine parts yielded **60 of 97**. Selecting
individual scenarios leaves the other rows empty and full admission pending.

Scenario 5 has two independent open requirements. The protected macOS
executor refused the retained diagnostic directory and nested product
sandbox. The existing tests also generate some compiler output with shell
commands and use different bytes on the text and Code Mode paths. A genuine
local compiler and a consistent byte fixture across those surfaces are
still needed. Scenario 7 on this frozen revision lacks the newer test
that executes the original shell generation after canceling its competitor.
[Execution-continuity observations](execution-continuity-native-targets-2026-11-18.md)
· [Collecting independent scenarios](../design/upstream-alignment.md)

#### Reconciliation at 04:22 UTC on November 19

Private review `a98666774c` includes ALab main through `58910a73cf` and
the same upstream frontier `27e2227b2b`. Its own native gate admitted
all 118 intake decisions and the hook comparison. It requested 14 contracts
and 97 scenarios. The gate log has SHA-256
`86943128bc711b16e66790bc99e899fca1be3d97cbf94289da84d81be378f05d`.
Its exact package attempt ran dependency-inclusive certification with the
source-selected Trust 15698. The in-process procedural macros left zero
proved claims and one unknown. Packaging exited 1 with log SHA-256
`f86276465c02149f73101c91e05249c877ae14c5f873821f08ea90028bb86f5c`.
[Reconciled source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-a98666774c)

The release lookup at 04:23 UTC returned OpenAI stable 0.170.0. Development
main `b5d9f7116e` was 415 commits beyond ALab main and 297 beyond the
review. Signed package index 165 verified against roster 5 and retained
Codex build `2026111701` and Trust build 27009 for macOS ARM64. Its SHA-256
is `efd935b7fd6233c7863f867e96aed057950d24f80c1f892da1ad1ab1ef85845f`.
[OpenAI 0.170.0](https://github.com/openai/codex/releases/tag/rust-v0.170.0) · [Signed index 165](https://github.com/alabsystems/aterm/releases/tag/atpkg-index-165)

Three cross-project exec controls in `299afea85b` began in B’s sibling
directory and explicitly selected A’s Git checkout with `exec -C`. Each
continued the original thread with B’s synthetic credentials, observed
the original project in a new shell call, retained both recorded turn
directories and executed each account’s command once. The official 0.170,
signed ALab, and signed ALab with separate active auth files produced
result SHA-256 prefixes `f2163ca3614e`, `be9aeedaf272`, and
`d1e75be7d93b`, respectively.

Work originating in `codex exec` also reopened interactively by UUID on
signed ALab 0.11.0 and official OpenAI 0.170.0. Two signed-to-enhanced
picker trials on `aeb6e36ed7` first showed an empty result under B with
**All local** and the default origin filter. Adding
`--include-non-interactive` exposed A’s thread. The cross-project trial
also used `--all` and selected the session directory. Each resumed the
original UUID, retained its working directory and original command output,
and executed B’s new command once. Commit `f76047a42d` contains the fixture.
The signed ID, vanilla ID, shared-directory picker and cross-project picker
result SHA-256 prefixes are `700e27df3996`, `02172d5830e9`, `01730a58b7f3`
and `592d2365e5d9`.

The review binary selected from Cargo’s `codex-cli/codex` artifact had
SHA-256 `9a551d8c0c59730aca06a37993f92059550621bf20be3287d2401102f7124be5`.
The comparison with official 0.170.0 passed all 18 checks across 77 help
pages and 26,145 shared schema entries. All six help differences and 244
schema differences were attributed. The comparison log has SHA-256
`b0b171e382dec2ff85b4254992ff84793a3a40e6fe61976faf6fff7d7ef79f39`.

| Signed ALab seed under A → `a98666774c` continuation under B | Result SHA-256 prefix |
| --- | --- |
| Terminal → interactive UUID, separate credential files | `5a91efb077b9` |
| Terminal → bare picker, shared directory | `7752f289b490` |
| Terminal → cross-project interactive picker, destination renewal | `b56928a32e4b` |
| Terminal → exec UUID, separate credential files | `4d11fd99d0b7` |
| Terminal → exec UUID from B’s directory with `-C`, destination renewal | `8a8a6b149fdd` |
| Exec → cross-project interactive picker, default-origin negative and explicit inclusion | `093c87f8d447` |

Both expired-destination cases renewed B exactly once, saved the new
credential and kept A’s credential intact. The cross-project trials
verified the restored directory through a second shell call. The result
summary has SHA-256
`53f7a7e9233ba2c4d9d3fd84180fa3fdade10e61341b12fe5974afe8eb0b0c17`.

The protected execution-continuity part covered scenarios 1–4, 6 and 7.
Eight named results passed on this source across the Python wire suite,
CLI fork-history integration tests and the Rust snapshot test. Scenario 7
executes the first generation’s captured shell value
after cancellation of its competitor. The part has SHA-256
`c743ce6238c91fcdcbcd3f2a8def97ccf092b460055f1215bb18369001c7288f`.
Execution-continuity scenario 5 still requires genuine compiler diagnostics
and a permitted executor for retained files across process replacement.

The CLI build also emitted three unused-import warnings at ALab’s retained
callback, pager and timer hooks. These hooks predate the review. The full
gate stopped before its lint step, so its verdict on this revision remains
unknown. The existing hold on full-capsule receiver activation still applies.
[Retained UI review](retained-app-transcript-ownership-2026-11-17.md)

All nine complete contracts and the two partially covered contracts were
collected again on `a98666774c`. The verifier read every part and test log,
checked the source, inventory, selectors and test outcomes, and found
62 covered rows. The 35 full-admission errors correspond to the 35 empty
rows. The combined receipt manifest has SHA-256
`63aaf414e54b75a5c67d8b18a9d99b262a261514f08eed55f3103047555cbc76`.

| Contract | Verified scenarios | Distinct named checks in the part | Part SHA-256 prefix |
| --- | ---: | ---: | --- |
| execution-continuity | 6 | 8 | `c743ce6238c9` |
| account-isolation | 1 | 7 | `7c9763acbd4d` |
| build-and-adoption | 4 | 128 | `34fe6828d654` |
| history-pagination | 4 | 8 | `ebb1d936cbec` |
| model-discovery | 8 | 40 | `6a6bd6ecf558` |
| native-ui | 5 | 15 | `ca1a85bfffe8` |
| product-policy | 3 | 23 | `351a870086be` |
| provider-recovery | 23 | 205 | `3eec1391f331` |
| qualification-fixtures | 6 | 34 | `297bfff74561` |
| security-notices | 1 | 13 | `5fbcc1c4e7ed` |
| skill-discovery | 1 | 2 | `41015d5a028c` |

The remaining rows are account isolation (6), exec handoff (10),
execution continuity (1), goal storage recovery (7) and native session
(11). The complete campaign refused these five contracts. The account
transport part used the dedicated collector from `cbb8244dcd` on the same
source and native gate record as the other ten parts.
[Account transport controls](account-transport-switch-2026-11-19.md)

The account-isolation audit on `a98666774c` left scenario 2 pending.
Existing tests refuse an unavailable refresh owner and preserve the
credential file. The supported assignment change restarts the host.
A qualifying test still needs to hold A’s refresh across that restart,
observe the file lock during persistence and check B’s first authenticated
read. The collector rejected scenario 2 without a matching recipe.
[Refresh tests](../../codex-rs/alab-core/glue/auth_refresh_lock.rs) · [Assignment lifecycle](../design/accounts-assignment.md)


At 05:30 UTC OpenAI development `524e15989b` was 418 commits beyond
ALab main’s upstream frontier `ce3031018e` and 300 beyond the review.
OpenAI stable 0.170.0, prerelease 0.172.0-alpha.1 and signed ALab 0.11.0
were unchanged. The macOS ARM64 Codex and Trust manifest SHA-256 prefixes
verified under signed index 165 were `2f494d9aec40` and `9db8f5ca5b2b`.
[Development comparison](https://github.com/openai/codex/compare/b5d9f7116e...524e15989b)

The scenario collector’s integrated Python suite ran 186 tests with one
optional native test skipped. In its separate qualified run, two native
selector startups timed out before the test sent a cancellation signal.
Commit `261f31f20f` retains the selector’s bounded diagnostics on failure.
On a clean checkout of that commit, the qualified publisher passed all three
fixture tests in 41.590 seconds, including native stage termination and both
collector termination modes. The result log has SHA-256
`524e72e2406d04247ad4f1739f413a930a89764f6e977969eca3a0a0a65ad61a`.

### Remaining acceptance work

The sync at `96042e0c18` integrated main through `953513eef6` and retained
the upstream frontier `27e2227b2b`. Its footer resolution preserves the
read-only transcript's account-row behavior and the newer quota-row width.
The native gate recorded 118 reviewed commits, accepted the hooks and exited
1 at the missing complete bundle. The gate log has SHA-256
`7f5141d26a95ece073018b6ce74e08e70604b739f064d5bb406668ab3485ab09`.
Its own campaign request recognized nine of the 14 affected contracts. All
97 scenarios remain in scope.

The `96042e0c18` TUI unit suite stopped compiling because three fork-owned
tests called a removed upstream API. Commit `b426390e7b` asserts the effective
permission profile for the configured modes. Candidate `fcb2205723` passed
those three native tests and three footer tests covering read-only history,
the weekly quota width and clickable account cells (log SHA-256
`40feae7fd72090d351179610a0145d923a1a41ef5cddc0237ea5f425eb2e1abb`).
The stable gate observation for that source uses base `1bf2f20e36` and
integrated upstream `27e2227b2b`. It reached the same missing-bundle decision
(log SHA-256 `0b30e68701f176c1a8d8f843e48aaeb015e0fd3519a0e955a58e12e99d2ed908`).

The combined recipe host at `6d12c616d6` recognized nine of the 14 contracts
in the frozen request. A full campaign on that host refused the following five,
which cover 42 scenarios. The seven declarations explicitly marked unqualified
belong to account isolation and live exec handoff. Complete the evidence at the
exact source selected for landing.

| Contract | Scenarios | Required observations to prioritize |
| --- | ---: | --- |
| Account isolation | 7 | Replace or retire an active credential owner during real reads. Exercise the retained login and managed-policy paths and observe closure of the old workers. |
| Exec handoff | 10 | Transfer a running parent and nested children with queued input. Interrupt at acceptance and release and observe who owns the work after recovery. |
| Goal storage recovery | 7 | Inject a lost acknowledgement after commit at native accounting paths. Observe listener retirement, cancellation and global queue pressure after committed set and clear. |
| Execution continuity | 7 | Reopen clipped diagnostics after a process replacement, with exact bytes, ownership and exit status. Exercise pending forks and cancellation of a competing shell snapshot. |
| Native session | 11 | Observe the aterm claim and mailbox under buffered replies, withdrawal and restart. Exercise native goal recovery, shutdown and resume against the real owner. |

Three further checks on fork main advance these incomplete contracts. The
goal queue exhausted its 128 global permits, admitted a different thread
when one slot reopened, and returned all permits after delivery. The shell
snapshot test executed the original captured generation after canceling a
second capture, then observed the fresh environment after owner release.
Two mailbox controls failed before the fix for ambiguous offsets and duplicate
local row IDs. Both passed after the fix alongside eight neighboring controls.
Each result identifies the exact feature commit in its decision record.
[Goal output queue](goal-output-order-2026-11-18.md)
· [Shell replay](execution-continuity-snapshot-replay-2026-11-18.md)
· [Inbox identity](native-session-inbox-identity-2026-11-19.md)

#### Goal storage recovery

On `bbd706f710`, four scenarios still need an observation that connects the
existing controls. The numbers below match the [scenario inventory](../feature_recipes/scenarios.json#L579).

| Scenario | What the existing fixtures exercise | Observation still needed |
| --- | --- | --- |
| 2. Lost accounting acknowledgement | A cancelled observer leaves its dispatched write running. A failed zero-usage stop holds admission. Separate tests distinguish definite noncommit from an ambiguous result and recheck a queued writer. | At each of the three native accounting entry points, commit the write and drop its acknowledgement. Observe the queued writer recheck admission and ensure the committed usage is counted exactly once. [Native accounting](../../codex-rs/alab-autopilot/src/goal_persistence_tests.rs) · [Accounting guard](../../codex-rs/alab-core/src/goal_persistence.rs) |
| 5. Status during a stalled read | An inline status snapshot reaches the outgoing transport while a goal read is stalled. Widget controls exercise the saved Active badge, input and writer activity. | Hold the same read across listener reattachment and observe Ready, Writing and Held at the live widget. Check that the saved Active badge persists and actual input and writer activity still register. [Transport](../../codex-rs/alab-autopilot/glue/goal_status_transport.rs#L558) · [Widget activity](../../codex-rs/alab-tui/glue/goal_runtime_status_widget_tests.rs#L171) |
| 6. Transport loss | The server tests cancel an enqueue and exercise bounded overflow, reset and rearm. The widget handles stale and duplicate packets and backend and thread changes. Its loss case injects `Lagged`. A separate loopback client test observes a physical socket close. | The physical close must reach the live widget. A later source carries the verified fixture described below. [Widget loss case](../../codex-rs/alab-tui/glue/goal_runtime_status_widget_tests.rs#L455) · [Loopback disconnect](../../codex-rs/app-server-client/src/lib.rs#L2146) |
| 7. Committed output order | A real observer disconnects after SQLite commits and a survivor receives the update. The 129-conversation control fills the global queue while writes are blocked. | Retire the same thread’s listener on each side of enqueue. Saturate and release the per-thread and global reservations with committed, undelivered outputs. Check per-thread order, response-before-notification order and independent progress on another thread. [Observer disconnect](../smoke/goal_observer_disconnect.py) · [Queue pressure and ordering](goal-output-order-2026-11-18.md) |

For scenario 6, the new fixture connects the widget to a loopback
WebSocket peer through the production remote app-server client. The peer
delivers `Held` for the selected thread. The fixture observes
the saved Active badge and a settled `Done` state. When the peer closes,
the production client emits `Disconnected` on that connection. The widget
enters reconnect, returns to `Working` and retains the saved objective,
Active badge and displayed thread ID. Four server tests also exercise
superseded queue writes, cancellation, overflow, reset and rearm.
[Physical disconnect fixture](../../codex-rs/alab-tui/glue/goal_runtime_disconnect_test.rs) · [Server queue controls](../../codex-rs/alab-autopilot/glue/goal_status_transport.rs)

All six named tests passed on fork source `8a6ba0ad71` and independently
on corrected 0.170 source `99c26f4361`. Both suites ran the four isolated
widget cases. A nonexistent selector failed on each comparison stage.
The receipt SHA-256 values were
`c6a925e7edac776d052f43c9a88c10348c7f0f8ab42d7326ed4973e294334f7e`
and `8a1080bd70a61f2edd9557b5fcde13b2e90e31797c0a9e6f94baea74fa3f6d2f`
respectively. Scenario 6 remains pending on the earlier frozen candidate,
which predates this fixture.
[Corrected 0.170 comparison](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-99c26f4361)

#### Account-isolation implementation work

A source audit compared `aeb6e36ed7` and main `ace8b3157e`. The retained
backend checks its physical connection and reads Input admission again after
the Native origin observation. The smallest missing runtime check is to hold
that observation pending, withdraw Input, release the observation and verify
zero new backend credential reads. Separately check that an already admitted
reader finishes its cleanup. The existing malformed-auth and unbound-reader
tests do not exercise that timing.
[Reader admission](../../codex-rs/alab-core/glue/retained_reader_runtime.rs)
· [Existing controls](../../codex-rs/alab-core/glue/retained_reader_runtime_tests.rs)

The retained frontend already consumes policy associated with its backend.
The source audit found that the old frontend's retirement helper has a
definition and a public re-export, but no invocation at production departure.
Connect the old frontend's reader census and unused startup seed at that
transition, then observe a real admitted reader drain and the seed receive
Native Closed before the replacement continues. Fresh-CLI credential tests
exercise a separate lifecycle.
[Frontend retirement](../../codex-rs/alab-tui/glue/owned_local_entered_readers.rs)
· [Frontend entry](../../codex-rs/alab-tui/src/retained_frontend_entry.rs)

The full native gate also requires dependency-inclusive compiler certification,
the ALab tests and lints, and the hooked upstream tests. A signed release follows
that gate and gets its own installed-binary resume comparison.
[Feature contracts](../features.toml) · [Goal recovery measurements](goal-output-order-2026-11-18.md)

While the 0.170 candidate was under test, main assigned ALab 0.12.0 to upstream
0.169.1 in `125674551e`. The signed package installed on this Mac at the time
of that change was still ALab 0.11.0, build `2026111701`. Allocate the 0.170
release number against the current ledger when the source qualifies.
[Version ledger](../versions.toml)

Integration commit `4fa8864f3f` starts from the private main that recorded
0.12.0. Its Git tree matches the previously reviewed merge at `a55e663e68`
byte for byte. The ledger accepted all 12 historical assignments. The
subsequent integration at `61ada23345` includes 407 reviewed upstream
commits through `d1d1ba0b83`. The release tool proposed 0.13.0 for upstream
0.170.0. Publication still requires compiler certification.
[Reviewed source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-61ada23345)

The signed 0.11.0 terminal reproduced the idle resume issue on conversations
saved with Sol 6.1 and Astra 6. Development build `d779970be2` restored
each transcript and displayed Sol 6.2 Ultra or Astra 6.2 Ultra, respectively,
from the first frame, even with the global default set to the other family.
Opening the conversation added zero turns. In separate runs, the first reply
used the model and effort displayed on screen. All 18 runtime checks passed
on that executable. The first-frame correction landed on private main in
`ddafc97279`.
[Terminal change](https://github.com/alabsystems/codex-alab/commit/ddafc97279)

Native index and package verification confirmed signed index 164, signer
`m22`, roster sequence 5, and index SHA-256
`ec8bc4458bb862ed1916b814f6ab5f09beda87be30130b7cc059a09138d328b6`.
It selects Codex build `2026111701` and macOS ARM64 Trust build `27009`.
The installed Codex tree matched its signed manifest and identified source
`70d6391259`. The 0.170 integration still pins Trust `15698`. Its exact
feature-gate record has SHA-256
`d7e15c0b50a375ab7e8133fd16688f9efb71f344045ce3643db260e8e7319823`,
exit status 1 and no qualified source. Trust 15698 refused in-process
procedural-macro dependencies during packaging. The adoption check on 27009
refused required `anyhow` build-script execution.
[Signed index](https://github.com/alabsystems/aterm/releases/tag/atpkg-index-164) · [Compiler adoption review](trust27009-adoption-build-script-refusal-2026-11-16.md)

A fresh package attempt named the exact private review source `fcb2205723`
and used `--any-commit --no-promote` with the source-selected Trust 15698
compiler. The compiler applied `policy=certify`, included dependencies and
refused the CLI's in-process procedural-macro dependencies. The certification
report recorded zero proved claims, one unknown and an inconclusive verdict.
The package command exited 1. Its complete log has SHA-256
`7a6b65040359fcb18eabb50121bcfcb9f5f5c92356d21478acd514bde6a76538`.
