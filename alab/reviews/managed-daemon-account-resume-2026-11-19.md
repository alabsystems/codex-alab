<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Bare resume with an already running local daemon

On source `bf31719ce3`, replacing shared A's credential with B's on disk
left the existing daemon using A for both inference requests in the resumed
turn. The B control conversation used B, and both account rows were visible.
On this source, discovery succeeded while model request ownership failed.

Each run used a fresh home, synthetic file credentials and a loopback TLS
provider. A and B shared one email address and had different user, membership
and workspace IDs. Signed 0.11.0 seeded the original thread with `codex exec`.
The candidate created B's independent conversation in a second project.
Two real terminals exercised the literal `codex resume` invocation and its
account and directory controls. The fixture explicitly enabled
`features.daemon_auto_start` for the managed-daemon path.

| Daemon's original login | B's credential source | Where the original thread became loaded | Resumed inference | Result |
| --- | --- | --- | --- | --- |
| B | Shared B | Original daemon | B twice | Passed |
| A | Shared file replaced with B | Original daemon | A twice | Failed |
| A | Separate named B file, shared A preserved | Separate from the original daemon | B twice | Passed |

The observer established a Unix WebSocket to the managed daemon. It read the
initial account and workspace, created an ephemeral thread and verified the
selected thread was absent from the loaded set. The same connection then
observed the loaded threads and original account while the resumed TUI was
live. The observed daemon retained its own ephemeral thread in every case.
The two model requests reported above came from the loopback provider's
actual bearer and workspace-header observations. Each daemon reported a
successful stop and released its private socket.

Use the [account resume smoke](../smoke/account_resume.py) with
`--surface bare-all-default --seed-surface exec` and
`--managed-daemon destination`, `--managed-daemon source`, or
`--managed-daemon source-to-named`. Pass the selected candidate executable
with `--codex`, the original release with `--seed-codex`, and a fresh short
path with `--output`. The observer uses Python's `websockets.sync` client.
The matching-login case requires the original daemon to load the resumed
thread. When the login differs the test accepts either serving path if the
observer retains its identity and thread and B supplies the resumed requests.

The candidate executable and the privately installed daemon copy had SHA-256
`b3fa8aec8bf58293baee6a5b1636725db8ae4441e93ed2da87e466959f90f32e`.
The private test layout used the signed 0.11.0 Code Mode host and ripgrep
as auxiliary inputs. The scenario requested ordinary shell execution.
The `result.json` SHA-256 values in table order were
`08b8cd3698dbcc17db1d8670457930fbf917774109093ca7c0a715227e0422c2`,
`1c51bf00a89c3b2b372d34ead36fb70edd3398246a7615dcdda92d5b7ad883ca`,
and `da485bb3104bf5e15072a45d4b7e617a09e12eb5c5fff559182afbbce359c8c4`.

## Verified correction

Fork commit `a66bc63fb2` and its 0.170 review `14be20c99e` compare the
launching managed login, user and selected workspace against the actual
local daemon connection. The comparison reads the cached account session
list without contacting the provider for that RPC. Initializing an ordinary
connection can separately schedule a server-side workspace-routing read.
A matching daemon serves the
original thread. B starts its own local server when the daemon belongs
to A, so A’s existing live thread can continue on its original daemon.
An API key or other source without a verifiable managed identity takes
the embedded path on implicit startup. Explicit remote targets keep
their selected backend.

All three cases passed on `14be20c99e`. The provider saw B on both
resumed requests. When the daemon belonged to A, the observer retained
A’s workspace and live thread. When the daemon belonged to B, the observer
saw the selected thread load on that daemon. The two named credential
files retained their original digests. CLI SHA-256 was
`976109f01b75714636d44ecc4bf59f556fc3c697ece17bdf4ed7176e7a5ccb5d`.

| Original daemon | B’s login source | Result JSON SHA-256 |
| --- | --- | --- |
| B | Shared file | `a46f88717e7b5bb1a6a1b8980500d3283b44d9a4c71c193d2fd1cf6082057e74` |
| A | Shared file replaced with B | `f387cb048fe78a4900c5c40d98118d76fa50d8590af4ebee2c55deb10fb0f16b` |
| A | Separate named B file | `6bd02606ee196967aa55add91cf4bc695e9f7dae41fcd889df704906086cfa66` |

The first named trial used a path longer than the Unix socket limit. It
stopped the fixture daemon before observing a resumed turn. The named row
above records the complete run in a new, shorter scratch directory.

## Reconnect verification

On review `79f89f3e92`, the matching B daemon and the named B credential
cases still passed. The shared-file A-to-B case exited before the picker
could render. Constructing a diagnostic for the expected login mismatch
installed the default error hook during preflight. The TUI then failed to
install its own hook. The provider saw no resumed turn. Separately, the
protected TUI test build reported older callers of the reconnect function
that still used its original signature. These observations kept this
intermediate source out of the final review. The shared-file failure
receipt has SHA-256
`1937bb3cbbf14565d635f506782dcd3b2244d535171739a155c0f6102b41926a`.

The corrected fork commit `166f453b2e` installs the interactive error
hook once before preflight. It keeps ownership failures typed until a
caller needs a diagnostic and preserves the original reconnect call shape.
Review `1b1eb96b99` passed all three daemon cases on the same signed
0.11.0 exec-originated thread setup. The provider observed B on both
resumed requests in each case. The previous A daemon retained its account
and unrelated live thread, and a matching B daemon continued the selected
thread on its existing connection.

| Final review check | SHA-256 of retained result |
| --- | --- |
| Shared A file replaced with B | `100a21392a8a2d1c480c394c95f37e5dddff21cfb73e087558ec8197c532c31b` |
| Shared file with B daemon | `3f2c9212a56183f267184930817031f0b42c5a7b7f003a124c643fe84ea2e687` |
| Separate named B file | `41635d3e082b415da0da7d7b23a07cde3e63af869f1c1f044b80c9d2781a36b4` |
| Bare interactive conversation | `979023fbc27a07c0abd48da65d45b63c4d1b93140fc71d5b98f6e39a5320247a` |
| Bare exec-originated conversation | `bba4e68a3beeef527ec050fa860ddf43d10ede8ebf769dc49a10c6f5b0fafa11` |

The CLI SHA-256 is `71ed8ec181c2f7e5ca58caa5966c47118957fb356bfc36d87093efe80ebe33b1`.
All 18 vanilla compatibility comparisons passed again.

All 138 protected native picker and daemon-ownership tests passed on
`1b1eb96b99`, including a physical socket replacement that presented
the same user and session with a different workspace. The rejected daemon
received no model bootstrap or thread-resume request. The native test log
has SHA-256 `b4355e9a0a803fec17b190ba4fd6114ec802e95806ceaf9708221653a1624672`.
The fork-main build independently tested both login storage paths. Its
CLI SHA-256 is `b49c316155cffe35fa8095f02c8a73117a36822e2c65967ffa7d64a0a0f3ea5f`;
the complete fork-main results manifest has SHA-256
`9f84d12a3ee09ec377ae08ede09b263d75ea121f85875b36b9751150a8ab25ed`.
[Exact 0.170 picker source](https://github.com/alabsystems/codex-alab/tree/alab/resume-default-0.170-1b1eb96b99)

A broader 144-test selection passed 143 tests in the standard runner.
Its remaining saved-opening test drains terminal input when it opens a
modal. The runner disconnects stdin. Supplying a real PTY to that exact
test process inside the same protected profile ran both its install and
reconnect paths successfully. The separately completed test log has
SHA-256 `8ec5307ff2509989eb5247ce20f65a7987a4ec9e712854053c928bbadb3c737e`.

Commit `cb3ba9bb62` gives the saved-opening test a private PTY and
requires its child process to finish the exact named case. On both fork
main and the 0.170 comparison `462a7405e1`, the standard runner with
disconnected stdin passed all 144 tests. Each suite's final success list
matched all 144 requested names. One live-rendered line in each combined
log was interleaved with terminal drawing. The saved-opening test also
passed independently on both sources. Full-suite log SHA-256 values are
`18b26595ec1e1a7fdcbc8b3b1c1e9f1ef78b1d4a0e9fdf0677ecd133378acc21`
on main and
`19241976bff0908e321956396585ee1bc9e82a5b3bffc65119b678f94a517adb`
on the 0.170 comparison.
[Saved-opening test](../../codex-rs/alab-tui/glue/saved_opening_wire_tests.rs)

## Crossing the first page under a different login

On the reviewed 0.170 source `bbd706f710`, signed ALab 0.11.0 created A's
conversation with `codex exec`. B then created 25 newer conversations in a sibling
project. The app server's first page contained all 25 of B's threads. Following
the returned cursor produced exactly one more row, A's original thread.

Opening plain `codex resume` as B initially showed row 1 of 25. Page Down
fetched the next page and moved to 23 of 26. Another Page Down selected row
26. Pressing Enter continued A's original thread in A's working directory.
The shell saw A's saved marker and both model requests used B's credential.
The two named credential files retained their original hashes.

Reproduce with `alab/smoke/account_resume.py --page-boundary --surface
bare-all-default --seed-surface exec`, a candidate and seed executable,
and a fresh output directory. The receipt includes the two RPC pages,
cursor, observed picker positions and terminal transcript. The result JSON
has SHA-256 `88b71038b656c783b52fc29cf4edb94d50059a703c8dad42998262bbeeb1e2f0`.
[Page preparation and checks](../smoke/account_resume_page.py)

## First conversation under a new login

Signed ALab 0.11.0 created A’s only conversation with `codex exec`.
Using B’s newly selected login in a sibling project, the **Current** login
filter showed “No sessions yet” and 0 of 0. A fresh, plain `codex resume`
opened with A’s conversation already selected at 1 of 1. Enter reopened
the original thread with its saved tool result and working directory.
Both inference requests and the account and model reads used B’s credential
and workspace. After the turn, A’s original rollout was still the only
rollout in the shared local home.

The candidate was the reviewed `bbd706f710` binary. Reproduce with the
account resume smoke’s `--empty-destination-login --surface bare-all-default
--seed-surface exec` options. The result SHA-256 is
`0d859c0e9b339be26100dedf4d418752867357b0004413e330f5fe5a3341e67a`.
The original two-conversation trial also passed with the revised screen
reader. The picker checks and their negative controls passed all 22 tests.
[Repeatable cross-login check](../smoke/account_resume.py)

## Cross-login resume at the OpenAI development tip

The private integration `56bc1f03a3` contains upstream `b9559ff737`,
all 422 commits since the fork’s mainline integration point. Signed ALab
0.11.0 created A’s conversation using `codex exec` in each of three
independent shared Codex homes. From another project, B then ran exactly
`codex resume` in a terminal. Each trial retained A’s thread ID, tool
output and working directory. Both requests in the continued turn
authenticated as B.

| Starting condition | Observed selection and account | Result JSON SHA-256 |
| --- | --- | --- |
| B had never made a conversation | The Current filter had zero rows. The initial All local picker selected A at 1 of 1. The original rollout remained the only one. | `e7ed51300e8de41f20e7a2d9f2bc62ce44bbee8c3f4cff9343a0584ca15dadad` |
| B had 25 newer conversations | The first RPC page contained those 25, the second contained A. Page Down reached row 26 of 26, and Enter selected A. | `0c8911d1ca2e273ba03bd099cc146fd9e5525470353d89c8b29b4497e96f9728` |
| A’s managed daemon and client stayed connected | The observer on the original daemon still read A’s account after B continued the thread. The daemon stopped and released its socket after the test. | `56808f35123352887a9a31ca915d43d6deaa8720b29a99afbb1b4a3cccb02f97` |

Four execution-wire checks passed on the candidate before interactive
acceptance. Its CLI SHA-256 was
`73b9d21225460249ee1c84c2e68c969da50c82a33dcf02771cc654cffc24ff95`.
The candidate used the signed package’s helper binaries. The acceptance
manifest records the exact executable, signed seed, smoke source, thread
IDs, JSON and terminal digests. Its SHA-256 is
`d0c6b6b92bd4d5e3103f75a0bb056a7e001ad6ac4610345a8814df6eea1b2d10`.

To reproduce, run the account resume smoke with the signed seed, candidate,
`--seed-surface exec --surface bare-all-default`, and one option per fresh
output directory. Use `--empty-destination-login`, `--page-boundary`, or
`--managed-daemon source`. The daemon observer uses the test environment
with Python’s `websockets` package. Use smoke revision `1571bdb4cd` or
later. OpenAI’s terminal probe now asks for DEC alternate-scroll mode on
startup. The screen reader at that revision recognizes the read-only query
and still requires a visible, completed picker paint. All three trials ran
the same unchanged CLI.
[Upstream terminal change](https://github.com/openai/codex/commit/a394ff9cf6) · [Interactive smoke](../smoke/account_resume.py)

## What the official 0.170.0 picker showed

Signed ALab 0.11.0 created a single exec conversation as A. In a sibling
project under B, the official 0.170.0 binary’s plain `codex resume`
reached a settled initial screen showing “No sessions yet” and 0 of 0.
The terminal exited normally. The official app-server identified B as
the current account. Its broad thread-list request returned A’s exact
thread in A’s original directory, as did a request scoped to that
directory and the exec origin. Starting from the broad query, restricting
only the account to B, only the directory to B’s, or only the origins to
interactive sessions each removed A’s row. These controls establish that
the history was present in the shared home throughout the trial.

Official tag `29eb79e986` supplied CLI SHA-256
`9248cfeebec9b4cc3034d5114caab10fbf426cbfa79dba410c5fa1b72762abd6`.
The result JSON has SHA-256
`d69c25dfaf5783de62105d0de0150c20b2bb74a35a836fff0f3f13177501fe16`.
The raw terminal capture has SHA-256
`65ead0c5453d9a90cd46ba9f2c90cd6cbb2d704552199649dd8a634768c29924`.
[Official release](https://github.com/openai/codex/releases/tag/rust-v0.170.0) · [Official picker source](https://github.com/openai/codex/blob/29eb79e9866b3262a777fdf8195c1b9e63684c91/codex-rs/tui/src/resume_picker.rs)

The committed `official_resume_picker.py` reproduced that frame byte for
byte in a second private home. Its six independent list controls passed
and its private daemon stopped after the measurement. The fresh result
has SHA-256 `4adc224473538079a1e254d3e0d78728f568f59215228bffdc8c16736d55d532`.
[Repeatable official baseline](../smoke/official_resume_picker.py)

## Cross-login resume after the gRPC host migration

The development comparison `1065c63abf` includes OpenAI’s `6271f35f52`
thread-start change and the gRPC-only Code Mode transport. The test layout
used the CLI and helper from this exact source. Signed 0.11.0 remained
the seed executable. On each of three real terminals B reopened A’s
original thread with plain `codex resume`. Retained requests verified
the saved tool history and B’s login on both calls in the continued turn.

| Starting condition | Result JSON SHA-256 |
| --- | --- |
| B had zero conversations. A was selected in the initial picker at 1 of 1. | `471558bc0b4e458a5a1f1a7c2ed53b0a622b4d7b9acf0080c3230e0243a4d6ce` |
| B had 25 newer conversations. A was selected at row 26 after paging. | `05fad8dbcb7a1e0492d6b1ea241bf1e8235070e0b24eff55405edaf01798527d` |
| A’s daemon remained connected to A while B continued in the original working directory. | `b4ff1155581b30f3976129e4aaa67b9509356383b3a7ac24b9bb3cadab3054cd` |

The result manifest binds both executable hashes, the test source, all
three thread IDs and the six terminal and result digests. Its SHA-256 is
`c6fb36d58b550b217a9517fa93f3638c3f6c0d47defb7fa966730a72fb32e767`.
[Integrated source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-1065c63abf)

## Searching across pages and renewing a first-time login

On the frozen 0.170 candidate `bbd706f710` and the development review
`1065c63abf`, B opened bare `codex resume` with 25 newer conversations.
The first page contained only B’s threads. Typing `cedar` found A’s older
exec conversation as the sole result. Enter continued the same UUID with
A’s tool history, in the original directory, and both new requests used B.
The public list API independently placed A’s UUID alone on page two.
Repeat with `alab/smoke/account_resume.py --surface bare-all-default
--seed-surface exec --page-boundary search` and the candidate, signed seed
and output arguments. The two-build receipt has SHA-256
`f04fd1e97ec8f9a6d455960f7349357741ec18ddd1250e5a3ca0ac758444e17f`.

Both builds also passed with B’s first login using an expired named
credential. The default picker showed A at 1 of 1, while the Current
login filter showed 0 of 0. B renewed exactly once, persisted the
renewed credential and authenticated both requests on A’s original
thread. A’s credential file kept its bytes, inode and modification time.
Repeat with `--empty-destination-login --separate-auth-files
--renew-destination` in place of the page option. The two-build receipt
has SHA-256 `4c31c262fc6129c4f7ce95b18ae9ef06de189775f219190e09c89ecdc04a219a`.
