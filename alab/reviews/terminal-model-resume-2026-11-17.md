<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Terminal model selection on resume and during a conversation

On November 17, 2026 the signed 0.11.0 build 2026111701 fetched a catalog offering
Sol 6.2, then sent the first request from a resumed Sol 6.1 thread to Sol 6.1.
The development CLI built from `a7724cc88e882b1f466fb081966716f28d897e97`
selected Sol 6.2 in the same exercise. The development executable had SHA-256
`6f682edd68ad77afb34ef88d8e051c853bd8f9ab590d02cd70a7790ac4a0d63b`.

Each case created and completed a real turn, closed the original process,
then started the CLI in a separate pseudoterminal with `resume <thread-id> <prompt>`.
A loopback Responses provider recorded the thread header, model, wire reasoning
effort and messages. Each terminal displayed the second answer. Both requests
in each case had the same thread ID, and the second included the original prompt
and reply. The Ultra preference mapped to `xhigh` on the wire in these catalogs.

| Executable | Local authentication fixture | Saved model | Catalog or selection on resume | Model on next request |
| --- | --- | --- | --- | --- |
| Signed 0.11.0 | ChatGPT | Sol 6.1 | Provider added Sol 6.2 | Sol 6.1 |
| Development CLI | ChatGPT | Sol 6.1 | Provider added Sol 6.2 | Sol 6.2 |
| Development CLI | ChatGPT | Sol 6.1 Ultra | Provider added Sol 6.2, global Astra 6.2 Low | Sol 6.2 Ultra |
| Development CLI | ChatGPT | Astra 6 | Provider added Astra 6.2 | Astra 6.2 |
| Development CLI | ChatGPT | Sol 6.2 | Provider added fixture-only Sol 6.3 | Sol 6.3 |
| Development CLI | ChatGPT | Sol 6.1 | Provider added Sol 6.2, explicit `-m gpt-6.1-sol` | Sol 6.1 |
| Development CLI | API key | Sol 6.1 | Bundled catalog | Sol 6.2 |
| Development CLI | API key | Astra 6 | Bundled catalog | Astra 6.2 |
| Development CLI | API key | Sol 6.1 | Explicit `-m gpt-6.1-sol` | Sol 6.1 |

The ChatGPT fixture used a local certificate authority and returned the loopback
TLS service as its workspace origin. It advertised the successor only after the
original process exited. The catalog cache and saved thread were left as written
by that process. The fixture-only successor exercises a model absent from the
binary's bundled catalog.

The signed executable had SHA-256
`090aacf5157ce4326da1744ab8869218dff31dc4fb7bde1f9bd226736f7022b3`.
The signed channel still selected build 2026111701 in index 163.

The maintained TUI regression completed all three bundled API catalog cases with
the development executable used below. For each case it completed a seed turn,
closed the app-server, launched the terminal with the saved thread ID, and checked
the model, reasoning effort and retained messages at the provider. The terminal
displayed the second answer in all three cases.

## Provider-declared successors

Three further runs used a configured gateway with API-key discovery enabled.
Its first catalog advertised only the fixture model `lab-blue`. After the seed
process exited, each new terminal fetched a catalog offering both `lab-blue`
and `lab-green` at the gateway's `/v1/models` endpoint.

| Gateway catalog on resume | Launch selection | First resumed request |
| --- | --- | --- |
| Blue declares Green as its successor | Follow the saved model | `lab-green` |
| Blue and Green are listed independently | Follow the saved model | `lab-blue` |
| Blue declares Green as its successor | Explicit `-m lab-blue` | `lab-blue` |

The loopback gateway observed both turns at its own `/v1/responses` endpoint.
In all three cases the second request carried the original thread ID, prompt,
reply and wire effort of `xhigh`. Each terminal displayed the second answer.
The source-selected development executable used for these runs had SHA-256
`9388447e936800c1cff42c63a075e2465b925d0afc86544b87ff03669b90530a`.

## An update during an open terminal conversation

Five runs kept one terminal process and thread open for two turns. The first
response carried a changed `X-Models-Etag`. The serving provider then received
the CLI's catalog request and advertised a newer model. After the terminal
displayed the first answer, the fixture typed a second prompt into the same
pseudoterminal. Each second request preserved the original thread, messages and
reasoning effort, and the original process displayed the reply.

| Executable | First request | Catalog change | Next request |
| --- | --- | --- | --- |
| Signed 0.11.0 | Sol 6.2 | Fixture-only Sol 6.3 added | Sol 6.2 |
| Development CLI | Sol 6.2 | Fixture-only Sol 6.3 added | Sol 6.3 |
| Development CLI | Astra 6.2 | Fixture-only Astra 6.3 added | Astra 6.3 |
| Development CLI with explicit Sol 6.2 pin | Sol 6.2 | Fixture-only Sol 6.3 added | Sol 6.2 |
| Development CLI with configured gateway | `lab-blue` | Blue declares Green as its successor | `lab-green` |

### One catalog shared by three open conversations

Signed 0.11 created three conversations at Ultra. One server reopened all
three before the gateway published Sol 6.2 and Astra 6.2. Two conversations
followed the saved family. The third explicitly pinned Sol 6.1 on resume.
The client then issued one ordinary `model/list`. All six turns in each run
used their original thread IDs and retained each thread's own messages.

| Reader | Following Sol | Following Astra | Pinned Sol |
| --- | --- | --- | --- |
| Official OpenAI 0.170.0 | Sol 6.1 | Astra 6 | Sol 6.1 |
| Signed ALab 0.11.0 | Sol 6.1 | Astra 6 | Sol 6.1 |
| Development `d779970be2` | Sol 6.2 Ultra | Astra 6.2 Ultra | Sol 6.1 Ultra |

The development server sent distinct `thread/settings/updated` notifications
to both following conversations before another inference request. On the next
turn, the gateway received its declared Ultra wire setting of `high` on each
successor and `xhigh` on the pinned original. The official binary returned
the two cached original models when asked for the catalog in this exercise.
The release check repeats the shared-server case on the candidate executable.
[Release check](../feature_recipes/model_wire_controls.py)

### Catalog changes from a second running server

Two servers shared a home with three conversations saved by signed 0.11.
One server fetched the new catalog while the other held the conversations.
Development build `fbc7e3c982` announced Sol 6.2 and Astra 6.2 to their
respective conversations before the next prompt. The explicit Sol 6.1 pin
remained in effect. On a separate run a Sol turn submitted immediately after
the peer received its catalog response used 6.2. When the provider withdrew
both successors before confirmation, the serving process checked again and
continued using each original model.

Development build `8c12251a4e` delivered both updates in a standard temporary
directory. In a home where the OS watcher delivered no events it sent the
next requests to the older models. The release check exercises both the idle
case and the immediate user turn. [Release check](../feature_recipes/model_wire_controls.py)

### Provider recovery after another tab discovers a release

A session saved by signed 0.11 received a peer's Sol 6.2 catalog update while
its own provider returned HTTP 503 to catalog requests. Development build
`fbc7e3c982` used Sol 6.1 during the outage and again on the first turn after
the service recovered. No further tab had written the cache.

On development build `bb2be7d017`, the same session used Sol 6.1 while
confirmation was unavailable and Sol 6.2 on the next turn after recovery.
An idle session independently received its updated model settings before any
new input. The server waits 30 seconds before retrying an unchanged candidate
in the background. New cache writes remain eligible on the ordinary poll.
Explicit pins and provider withdrawals also passed on this executable.
The CLI had SHA-256 `23dc6b9152685e1bf91fe6da12ddb889b7b353db18417d0ebe5c50767ceb805a`.
[Release checks](../feature_recipes/model_wire_controls.py)

### Hourly refresh in an idle conversation

Signed 0.11 saved a Sol 6.1 Ultra conversation. Development build
`d779970be2` resumed it while the provider still advertised Sol 6.1.
Three seconds later the test provider advertised Sol 6.2. The client sent
no input or model-list request while waiting. After 59 minutes and 57
seconds of wall time, the server fetched the catalog and sent
`thread/settings/updated` with Sol 6.2 and Ultra. The next user turn used
Sol 6.2 with its advertised wire effort of `high` and included the earlier
reply from Sol 6.1. Both saved turns kept Ultra and the same conversation ID.

## Noninteractive resume

The signed 0.11.0 CLI completed a saved Sol 6.1 Ultra thread through
`codex exec resume` after the global preference changed to Astra 6.2 Low.
Its next request used Astra 6.2 Low. The updated CLI used Sol 6.2 and retained
Ultra. A separate run with an explicit Sol 6.1 launch pin retained Sol 6.1
Ultra, and a simulated future Astra release advanced Astra 6.2 to 6.3. All
three development runs returned the answer on stdout and sent the original
thread ID, prompt and assistant reply to the serving provider.

The gateway fixture advertised a new successor only after the seed process
exited. The signed CLI stayed on the old model and sent Low. Both
`codex exec resume` and the interactive terminal used the successor with the
updated CLI. Its wire effort changed from `xhigh` to `high`, the gateway's
declared mapping for Ultra on each model. An explicit model pin retained the
old model and its Ultra mapping. An explicit Low effort kept model promotion
enabled and sent Low. The release check exercises both entry points with
separate persisted threads and isolated homes.

The development executable used in these comparisons had SHA-256
`49dc5be4aaa33f78992a199d29b7ed4cc02dab439e65730b6f52bae275cfc3da`.
The protected runtime probes completed successfully for both entry points.
Separate first-party catalog fixtures advertised successors for two additional
families. The same executable resumed Terra 6.1 on fixture Terra 6.2 and
Luna 6 on fixture Luna 6.1. In each case the provider received the original
thread ID, history and the saved effort.

## Release status on November 18 UTC

The release build of private main `ba70fd1682331e07599b3aa14bf883215667ac48`
selected the repository's pinned Trust build 15698. Its certification frontend
refused the CLI's in-process procedural-macro dependencies before it could
produce compiler verification results. The package command exited 1.
Signed index 164 continued to select 0.11.0 build 2026111701.

The roster signature on index 164 and the signature on the Codex package
manifest both verified under roster sequence 5. The installed archive and
payload tree matched the manifest's SHA-256 and tree root. The authenticated
payload's `alab-build.json` records source
`70d63912591840da2a154f057dc0cade90a17d79`, Trust 15698 and build observation
`e3a0d76caa948d9d7d1c7f49b1f75203b5cd60fa8e7337ebb8b4683a64d70957`.
At that source, `codex-rs/alab-release/src/product.rs` constructed
`targo --unverified build --release --locked` for the three shipping
executables, and the worker in `src/main.rs` enforced those arguments. That
observation records an unverified compilation with an authenticated compiler.
[Signed package](https://github.com/alabsystems/codex-alab/releases/tag/atpkg-codex-2026111701)

Commit `b35046b529d229bcb0887ca403ae6f323e2ac9c2` changed the native
release controller to build those executables through `alab/certified-build.sh`
and bind the certification report and executable bytes to assembly. The signed
index's newer macOS ARM64 Trust package was 27009. Its native adoption attempt
refused required verification of the `anyhow` build script's execution and left
the project pin at 15698. A shipping successor therefore still needs a compiler
that passes the project's dependency-inclusive certified build.
[Native adoption result](trust27009-adoption-build-script-refusal-2026-11-16.md) ·
[Signed index](https://github.com/alabsystems/aterm/releases/tag/atpkg-index-164)

The publication registry assigns regular release production to
`codex-release publish` on a rostered Mac. The generic scheduled publication
cycle skips this repository, and the fork's scheduled `alab/daily.sh` runs
source synchronization. At private main `c4d1d2f5be77`, the ledger ended at
0.11.0 on upstream 0.169.0. The official 0.170.0 branch point `72532ecae5`
was still outside its ancestry. Product versions can share a baseline. A
0.12.0 claim for the 0.170.0 integration belongs on main after that integration
lands, when the release ledger can record the intended baseline.

To resume an existing Sol thread on that signed build using Sol 6.2 Ultra,
either command accepts the original thread ID.

```sh
codex -m gpt-6.2-sol -c 'model_reasoning_effort="ultra"' resume "$thread_id"
codex exec -m gpt-6.2-sol -c 'model_reasoning_effort="ultra"' resume "$thread_id" "$prompt"
```

Both commands were exercised with the signed binary and an isolated provider
fixture. Each resumed a Sol 6.1 Ultra thread, preserved its ID and prior
exchange, and sent the next request on Sol 6.2 with wire effort `xhigh`.
The global setting in each fixture remained Astra Low. These flags explicitly
select Sol 6.2 for the invocation. On development commit `fb6884c384`, a
separate `codex exec resume` check without either flag also displayed the
saved Ultra effort in the startup banner and used the provider's successor.

## Selecting a saved thread

Signed 0.11.0 created nine separate threads using Ultra. In a fresh process
development commit `fb6884c384` resumed them with the interactive session picker,
`codex resume --last`, or `codex exec resume --last`. Each path advanced Sol 6.1
to Sol 6.2 and Astra 6 to Astra 6.2. Each also kept Sol 6.1 when the invocation
explicitly requested that model.

The loopback first-party API-key fixture recorded the original thread ID,
prompt and assistant reply on every resumed request. All nine sent `xhigh`
even though the fresh configuration requested Low. All three exec banners
displayed the model used on the wire and the saved Ultra effort. The resumed
CLI's SHA-256 was
`8d7fbda2fd4d761b52ef64a442af2fda236d54fd3b4ba98700b87aea0e915e27`.

## Recovering a saved effort after a catalog change

Private main `44b634d3a8` preserves the requested effort in the thread's
rollout. A temporary restriction changes the request for that turn. In the
Astra fixture the account first offered Ultra, then only Low. After the
second process exited, a fresh process resumed the same thread and sent
`xhigh` when Ultra returned. It also sent `xhigh` on the fixture's Astra 6.3.
The earlier executable continued sending Low after both changes.

Five app-server cases exercised live Sol, cold Astra, an explicit pin, and
live and cold sessions on a custom provider. Once the serving catalog showed
that the followed model and all eligible successors were absent, the client
sent no inference request. Retrying on the same thread found an eligible model
when the catalog recovered. The custom provider's declared successor was used
while an unrelated, higher-numbered model was ignored. A withdrawal between
refreshes can still cause the provider to reject the first request. The
verified development executable had SHA-256
`9690fa16cccc4f40cba9af43cc03993a58547660c0d718d8395d2b02ba88c6e2`.

## API-key sessions with an explicit OpenAI inference URL

Setting `openai_base_url` to the same OpenAI URL the CLI would otherwise use
changed model discovery. On development source `fb6884c384`, an existing Sol
6.1 Ultra thread advanced when the inference URL was implicit. Explicitly
setting `https://api.openai.com/v1`, or `https://gov.api.openai.com/v1` with a
FedRAMP key, left the same kind of thread on 6.1. Both matched implicit URLs
found the successor. The official 0.170.0 executable selected bundled 6.2
with either explicit URL and used the new global Low setting in all three
resume comparisons.

On ALab source `a788838355`, the same signed 0.11.0 executable created 16
independent Sol 6.1 Ultra threads for interactive and exec resume. After the
fixture published a successor absent from the binary, both matched endpoint
and key combinations found it with an implicit or explicit URL. Both surfaces
kept an explicit model pin, preserved a tenant gateway's selection, and
withheld promotion on either credential mismatch. Every second request
retained its thread ID, prior exchange and `xhigh` wire effort. Each exec
banner reported the model it sent. Ten fresh terminal and exec starts covered
both matched account types and an explicit model pin.

Eleven further cases exercised named provider ownership, administrator migration
rules, discovery opt-out and credential changes in an existing app-server.
A custom provider displayed as OpenAI retained its own catalog. Replacing a
commercial key with a FedRAMP key, then restoring it, hid and restored the
commercial successor. The reciprocal sequence worked on the government URL.
The captured discovery requests always matched the currently selected credential.
The TLS responders used fixture keys and advertised `gpt-6.3-sol` solely to
distinguish a provider release from the bundled models. The tested CLI SHA-256
was `6f1292833007b758f93dd6931d6ab93f7f95f770845a4e60a92464b3479e733c`.

With the Bedrock and Vertex changes integrated, development source
`601bce9fba` passed the same 37 OpenAI checks and the 52 cloud checks. All 89
ran on one executable with SHA-256
`65ac75400b4152d950631bc5d281aae1a3a21084f692705b26a78bf8a3e6a696`.
[Cloud cases](upstream-stable-0.170.0-2026-11-18.md#existing-cloud-sessions)

The maintained `alab/smoke/model_resume.py --seed-codex` comparison also
exercised an opaque provider-declared replacement. Signed 0.10.0 and 0.11.0
each created two threads on `gateway-amber`. With development executable
`601bce9fba`, both terminal and exec resumed on `gateway-indigo` and used
its mapping for the saved Ultra effort. Resuming with signed 0.11.0 itself
requested `gateway-amber` at the new global Low effort.

On the same development executable, six first-party resumes also read threads
written by signed 0.10.0. Interactive and exec resume both advanced Sol 6.1
to Sol 6.2 and Astra 6 to Astra 6.2. Both honored an explicit Sol 6.1 pin.
The captured requests retained the thread, prior exchange and Ultra effort.

Ten further cases started with a signed 0.11.0 thread and changed the catalog
while the development app-server was running. Each resumed thread completed
a turn on its old model. The fixture then added a newer family member and
continued offering the old one. After a normal `model/list` refresh, the next
turn used the fixture successor on both commercial and government OpenAI
endpoints with matching credentials, for both Sol and Astra. Each family also
kept its original model under an explicit pin, on a custom gateway, and with
a commercial URL and government credential. All ten retained the thread ID,
conversation history and Ultra effort through the second turn in that process.

The same ten cases also ran with automatic discovery alone. During the first
resumed response, the fixture published the new catalog and sent the normal
`X-Models-Etag` header. The test client issued only ordinary turn requests to
the app-server after resume. On the next turn both eligible account types used
the new family member for Sol and Astra. All six pin, gateway and credential
controls kept their old models, and all ten preserved the conversation and
Ultra effort.

Ten interactive terminal runs repeated that signal during a displayed response.
After the answer appeared, the driver typed a new prompt into the same terminal.
In all ten cases the final status line showed the model and Ultra effort used
by the next request. The four eligible Sol and Astra threads advanced. The six
pin, gateway and credential controls stayed on their saved selections. Two
commercial controls resumed with signed 0.11.0 itself and continued to display
and request Sol 6.1 and Astra 6 respectively. Each terminal exited normally.

## Existing sessions on the integrated upstream candidate

Signed 0.10.0 (build 2026111601) and 0.11.0 (build 2026111701) each
created four conversations with Ultra selected. The development executable
from `db0afcca2f`, incorporating official `a05444bbeb`, resumed each
conversation on its original thread. Interactive and exec resume selected the
successor on the first request. In the app-server and terminal cases where the
provider announced the successor during a turn, the following prompt selected
it without restarting the process. Each captured request retained the prior
exchange and used the successor's mapping for Ultra. Both terminal cases for
each release displayed the successor and Ultra before exit.

All eight runs used the maintained `alab/smoke/model_resume.py --seed-codex`
probe. The development executable had SHA-256
`469db8412046c9f3ce819aa2440915d964d155a9c290dec39e6f7811c5cc4218`.
The signed release channel served 0.11.0 during these runs.

## Older saved threads after a change in global preferences

Signed 0.7.0 (build 2026111301) created eight threads on Sol 6.1 or Astra 6
with Ultra selected. Before reopening each thread, the fixture changed the
global model to the other family and the global effort to Low. Source
`57da60bd6d` resumed each thread in the interactive terminal. When the
serving catalog already offered 6.2, the first request used the successor.
When the catalog changed during the resumed turn, the following request used
it. Four explicit model pins kept their original selection. All eight
conversations retained their history, displayed Ultra and exited normally.
The same eight cases passed with signed 0.10.0 creating the thread and
`db0afcca2f` resuming it.

The 0.7.0 manifest verified under the installed atpkg master key. Its executable
had SHA-256 `6bd514a71736c8995c7e9aa79e641d8bccc047ae15b5ae8865dd12172bddcfc4`.
The unstamped `57da60bd6d` executable had SHA-256
`276eb0e3875b51e14ec852339656e4b16b9a8b6f34df8455bdf6ad80429dd733`.
Its observed catalog request sent the source tree's client version `0.170.0`
and its product display remained `codex-cli 0.0.0`. An online `model/list`
with the configured account refreshed the disk catalog and listed Sol 6.2
and Astra 6.2 with Ultra.

## Two terminal clients sharing a loaded conversation

A signed 0.11.0 process created a conversation at Ultra. A terminal resumed it
on a separate app-server listening on a temporary Unix socket. After its first
reply, the provider added an eligible successor without an ETag or notification.
The first terminal stayed open while a second terminal resumed the same thread
and submitted the next prompt. All four runs fetched the changed catalog, kept
the original thread and both prior replies, and exited cleanly.

| App-server | Terminal clients | Model on the next request | Both terminal status lines |
| --- | --- | --- | --- |
| Signed 0.11.0 | Signed 0.11.0 | Original | Original, Ultra |
| Signed 0.11.0 | Source 57da60bd6d | Original | Original, Ultra |
| Source 57da60bd6d | Signed 0.11.0 | Successor | Successor, Ultra |
| Source 57da60bd6d | Source 57da60bd6d | Successor | Successor, Ultra |

The server version determined the model used for inference. The provider
received the original model's wire effort `xhigh` or the successor's `high`;
each represented the conversation's saved Ultra preference. The second TUI's
startup fetched the catalog in this configuration. A separate bare
`thread/resume` against the already-loaded source server made no catalog request
and continued on the old model, identifying the additional server-side refresh
needed by clients that reattach directly.

## A release between attachments to one server

A signed 0.11.0 CLI created a conversation with a loopback provider. A development
server resumed it, completed a turn, and stayed running while the provider silently
published a successor. The client then issued an ordinary `thread/resume`. It
did not request the model list. Each run checked the model returned by the
server and the next inference request, including its thread ID, earlier messages
and reasoning effort.

| Development source | Thread state at second resume | Next provider request |
| --- | --- | --- |
| `57da60bd6d` | Loaded | Original model |
| `74bbb8c1a4` | Loaded | Successor |
| `74bbb8c1a4` | Archived during the pending catalog read, then restored | Original model |
| `292a65c196` | Loaded, unloaded before release, or restored after a concurrent archive | Successor in all three cases |

The unloaded case checked that `thread/loaded/list` was empty before the release.
The previously confirmed catalog in the server had outlived the loaded thread.
On `292a65c196`, both a loaded attachment and a cold resume in that server
fetched the new catalog. The gateway received `high` for the successor, its
advertised mapping for the saved Ultra preference. An inference already in flight
finished once on its original model. The following inference used the successor.
An explicit model pin kept the original, and an explicit Low effort still allowed
promotion. A failed catalog request preserved the working selection.

The development executable had SHA-256
`8823b496dad613351e6a8c235fde1aea02208864e5c865b53de1d5daa9b59f5d`.
The release runtime check exercises both forms of reattachment on the actual
app-server. It also checks closed-process exec and terminal resume, and a catalog
change observed during an open app-server or terminal conversation.

Threads created by signed 0.7.0, 0.10.0 and 0.11.0 also passed the two
reattachment cases against first-party catalog fixtures. Each release covered
Sol 6.1 and Astra 6 with and without an explicit model pin, eight cases per
release. The provider received each unpinned conversation's first post-release
request on its family's 6.2 model with the original history and Ultra effort.
The explicit pins retained their original models.

A separate check used the configured account's live model service on November
18. Signed 0.11.0 created isolated Sol 6.1 and Astra 6 conversations with a
loopback inference provider. On reopening, the signed reader and the
development reader each called `thread/resume` before requesting the model
list. A cold resume and a second attachment both returned the following models.

| Model saved with Ultra | Other global preference in the isolated home | Signed 0.11.0 reader | Reader at `292a65c196` |
| --- | --- | --- | --- |
| Sol 6.1 | Astra 6.2 Low | Sol 6.1 Ultra | Sol 6.2 Ultra |
| Astra 6 | Sol 6.2 Low | Astra 6 Ultra | Astra 6.2 Ultra |

Each reader then listed both 6.2 models with Ultra available. The account
service populated the isolated homes' catalog caches between 17:38 and 17:40
UTC, using client version 0.170.0. Only the catalog requests used the
configured account. The inference checks above used the loopback provider.

## New conversations in a server that stays running

With the model and effort both left at their packaged defaults, one app-server
started two independent conversations. The first-party fixture initially offered
Sol 6.1, then silently added Sol 6.2 between the two starts. On `4463f1a500`,
the first request used Sol 6.1 Ultra and the second used Sol 6.2 Ultra. This
passed with the default inference URL and with the canonical URL explicitly
configured. The same two runs with an explicit Ultra preference also passed.
On `292a65c196`, both starts had used Sol 6.1 with that preference.

The updated development executable had SHA-256
`a0f542d1e5a72241922cf01bfa1eb48e59f339c1b58961ae1954f6edbc43c8c5`.

## Resumed turns on the configured account

On November 18, the signed 0.11 executable created two test conversations in
isolated homes. The 0.12 development executable at `d9cbdf2e43` resumed each
one after a fresh server start and again in that same running server. Both times
it sent a turn through the configured account and received the requested reply.

| Saved conversation | Global setting during resume | First answer after restart | Second answer in the same server |
| --- | --- | --- | --- |
| Sol 6.1 Ultra | Astra 6.2 Low | Sol 6.2 Ultra | Sol 6.2 Ultra |
| Astra 6 Ultra | Sol 6.2 Low | Astra 6.2 Ultra | Astra 6.2 Ultra |

Both conversations kept their original thread ID. All four turns completed
without a model reroute. The same development executable passed the 14 local
regression cases, including concurrent resumes, explicit pins and archived
threads. Its SHA-256 was
`c9f89c1e310a7367b2f789deea3f302cbcb6b70b805f6a7e3914a4210ff5f10b`.
The signed channel still served 0.11 at the time of the check.

## Fresh threads on both enrolled subscriptions, November 18

The managed 0.11.0 package (build 2026111701) on m159 contained Sol 6.2 and
Ultra as its packaged defaults. The user's top-level model and effort had
selected Astra 6.2 Low. After removing those two settings, two concurrent
installed app servers used the same normal Codex configuration and separate
enrolled credential slots. Each returned its expected email from `account/read`.

For each account, `thread/start` with only the project directory and
`ephemeral: true` returned `gpt-6.2-sol` and `reasoningEffort: ultra`.
The paginated `model/list` response from that server showed Sol 6.2 as the
highest numbered visible Sol offering Ultra. Neither thread submitted a turn.
This checks effective startup settings even for a subscription whose weekly
allowance is exhausted. Selection of a future catalog release on a running
server is a separate source-build check described above.

The package drop-in check now compares a fresh thread's returned effort to
the default declared at the package's source commit. This catches a fallback
to Low after configuration loading. Signed 0.11.0 passed the new check in a
scratch home without a subscription login. In the comparison with vendor
0.169.1, the separate app-server schema comparison failed: of 889 schema
differences, 269 lacked attribution. The overall comparison remained failed.

Rechecking the same packages identified a stale local upstream ref
(`4688e49329`). Fetching upstream restored the signed package's merge base,
`ce3031018ef`, and reduced the unexplained count to 29. Six generated rows
refer to two ALab notification methods declared in the package source.
The other 23 refer to `AgentMessageDisplay.action`: the vendor release's
experimental exports include the field and the upstream base's exports omit
it from every variant of that type. With those checks the schema comparison
passed twice, covering 23,057 shared entries and all 889 differences.

Both reruns passed the fresh-thread Ultra check and failed the separate
`CODEX_HOME` comparison. Only the ALab scratch home retained
`shell_snapshots/<id>.tmp-N` after exec. That path is still reported by the
comparator; the cause of its retention is unverified.

### Shell snapshot timing on the signed packages

Five paired loopback exec runs on m159 alternated which package ran first.
All five signed 0.11.0 runs left a temporary snapshot after successful exit;
the file was still present half a second later each time. None of the five
vendor 0.169.1 runs left one. Two additional 0.11.0 runs that delayed the
mock response by half a second left no snapshot; both immediate-response
controls left the temporary file. The home-layout comparison still includes
these paths.

A core regression test first publishes one generation, then starts another
and waits until its validation shell signals that validation has begun.
Cancelling that task on `7bbe638d22` left the second temporary
file beside the first published snapshot. The original success and separate
generation tests passed on the same revision.

The fix gives the blocking writer ownership of the temporary file while it
writes, then passes ownership to the validating task. Publication is
synchronous and refuses an existing destination. This closes the cancellation
window without waiting for snapshot capture at every exec shutdown.

All 23 core shell snapshot tests passed with the change on m159, including the
previously failing cancellation test. A separate collision test verified that a
failed staging or publication attempt preserves the existing file.

The normal CLI produced by the source-selected Trust 15698 integration test
completed seven loopback exec requests, five with an immediate response and
two with a 500 ms delay. Every run staged and published a generation and left
the snapshot directory empty. With the same monitor the signed ALab package
left a temporary file on four of five immediate responses, and the vendor
package left none in seven runs. The development CLI finished validation
before shutdown in all seven runs; the core regression exercises cancellation
at a fixed point. The test executable had SHA-256
`6de96dab64afe0ee8a81a33178ef05b1bcf3d347837e593cf0da7ad30e7f5278`.
These checks used an unverified-lane integration build. The signed package on
m159 is still 0.11.0.

Three further paired trials waited until the actual zsh validation process
had opened the staged generation before releasing the loopback response.
All six CLI processes returned success with a completed turn, before a final
snapshot was published. The signed ALab CLI left the temporary file in all
three trials. The patched CLI removed it in all three.

## Review model selection on a conversation that follows Sol

On the development CLI at `acfa475f44`, a loopback catalog offered Sol 6.1 and
Sol 6.2. A conversation following Sol started on 6.2 Ultra. With
`review_model = "gpt-6.1-sol"`, the public `review/start` operation sent the
review request on 6.1. The parent's next request still used 6.2 Ultra. With
the review setting omitted, both requests used 6.2. The same explicit setting
on the earlier `d9cbdf2e43` development CLI had sent the review on 6.2.

The native handoff tests also exercised an explicitly pinned first review,
an inherited first review, manual compaction and the first ordinary user turn.
All five selected tests passed after the review delegate cleared its inherited
family-follow setting. The public-test executable had SHA-256
`61d2c460cd59f316f41ebd78a1276216a846988c5f8043938a290d703aa88585`.

## Choosing a saved conversation without its ID

On November 18, three isolated terminals used the signed 0.11 CLI (build
2026111701) to create conversations. Each sent an initial prompt and received
a reply before exiting. A loopback provider then advertised a successor. The
global default changed to a different model with Low effort. Each development
CLI invocation selected a conversation without receiving its ID.

| Entry point at acfa475f44 | Observed continuation |
| --- | --- |
| Interactive `codex resume` | The picker showed the first prompt. Selecting it restored the original transcript. The next reply and status line showed a successful continuation on the successor with Ultra. |
| Interactive `codex resume --last` | The resumed turn completed with the successor and Ultra visible in the terminal. |
| `codex exec resume --last` | The command printed the expected reply and reported Ultra in its banner. |

In all three runs the provider received exactly two inference requests. The
second used the original thread ID and included the original prompt, its reply,
and the new prompt. It selected the advertised successor and that model's wire
setting for Ultra. Both interactive processes exited normally. The candidate's
SHA-256 was `61d2c460cd59f316f41ebd78a1276216a846988c5f8043938a290d703aa88585`.

The maintained runtime gate now exercises all three selectors. On one early
live-terminal run, typing immediately after the answer appeared steered the
still-open turn. The provider used the original model twice on the same turn
ID, and the rollout had no intervening completion. A clean terminal exit also
proved insufficient to show the last turn had completed.

The check now waits for a matching completed turn in the saved conversation
before typing again or closing the PTY. It compares the turn IDs in outgoing
provider requests and verifies each terminal completion against that ID. Signed
0.11 to development build `17f6dd5f2` passed all nine resume cases, including
the interactive picker and both latest-session selectors. Five further live
terminal runs each recorded two distinct completed turns after resume. The
first used the original model and the second used the advertised successor.

## Latest conversation across two workspaces

Signed 0.11 created Sol 6.1 Ultra in one directory, then Astra 6 Ultra in
another, under the same isolated Codex home. Both terminals completed a turn.
The prompts and replies identified their workspace independently of the thread
ID. Each case started from a fresh pair. The global preference at resume was
the opposite family with Low effort.

| Command from the Sol workspace | Conversation selected | Next request on the 0.12 development CLI |
| --- | --- | --- |
| `codex resume --last` | Local Sol | Sol 6.2 Ultra |
| `codex exec resume --last` | Local Sol | Sol 6.2 Ultra |
| `codex resume --last --all` | Newer Astra | Astra 6.2 Ultra with either working-directory choice |
| `codex exec resume --last --all` | Newer Astra | Astra 6.2 Ultra |

The gateway advertised each successor after both original sessions had closed.
Every resumed request carried the selected thread ID, its original prompt and
reply, and a new turn ID. Each interactive run recorded completion before
exiting. In the cross-workspace directory prompt, the selected session and
current directory options each produced the requested directory in the saved
turn context while preserving Astra.

A separate comparison used the bundled model catalog. Signed 0.11
`exec resume --last` and `--last --all` chose the correct transcripts but
sent the opposite global model at Low on both next requests. The development
CLI also passed the bundled local case on Sol 6.2 Ultra. The development
executable was `e317f1333a`, SHA-256
`484dd82a613273a91b03458e5cd1043b9d2d523cedfff1b503d1a4059b15b5a6`.

The release check also exercises both exec selectors consecutively in one home.
It creates a terminal thread on `gateway-amber` with Ultra, then a newer thread
on `gateway-ochre` with High in another directory. After the gateway advertises
`gateway-indigo` as the successor to amber, the global effort changes to Low.
On the development executable `e8355ffcb2`, `--last --all` continued ochre at
High, then `--last` continued the original thread on indigo with its Ultra wire
setting. On signed 0.11, both commands sent Low and the second command sent
the local prompt to the ochre conversation. Each observation used the provider
request's thread ID, turn ID and history.

### Live Work catalog after resuming a signed session

Two Work clients shared a home. The first resumed a conversation written by
signed 0.11. The second discovered Sol 6.2 and wrote the Work model catalog.
The default catalog still advertised only Sol 6.1. Before the fix, the first
client sent its next request on Sol 6.1. Development CLI `256d9bf094` sent
that request on Sol 6.2 with the same thread ID and its earlier conversation.
An idle client also observed the change before the user typed. Each serving
manager confirmed the successor with its own provider.

All 12 model wire controls passed on that executable, including pinned models,
independent Sol and Astra threads, and recovery from a temporary provider
failure. The executable's SHA-256 was
`d0902ad4a19ad93fd9d99c0594f73a392764cb4f89ec827a934385aa4cb5efae`.

### Rechecking sessions saved by 0.11 on the 0.13 development binary

The 0.13.0 development CLI at `eab7cc5e99` passed the signed 0.11
predecessor's compiled target reader and resumed an original terminal
conversation. In the two-process catalog test, Sol and Astra threads saved
by 0.11 both moved to 6.2 at their saved Ultra preference. The provider
received the successor on the first input after publication. An explicit
Sol 6.1 pin held. When the serving provider withdrew the new releases,
the next requests kept their previous models. All 12 model request tests
also passed on this CLI (SHA-256
`f0c9be491b4cb73920b54bca8fbec75cf1216bb361ae6192fb77b2182a6bb876`).
The signed index still selected 0.11.0 during these runs.
[Development source](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-eab7cc5e99)

### Two successive upgrades of the same saved conversation

The signed 0.11 CLI created three conversations. Development builds 0.12
(`248ff99eca`) and 0.13 (`eab7cc5e99`) each continued the same conversations
with ordinary `codex exec resume <id>`. Sol 6.1 and Astra 6 advanced to 6.2.
After the test provider removed those original catalog entries and advertised a
fixture-only 6.3, both existing conversations advanced again on their first turn.
Each kept its Ultra preference through a temporary High limit on 6.2. A third
conversation explicitly pinned to Sol 6.1 on both resumes kept Sol 6.1. All
nine provider requests carried the original thread ID for their conversation
and a distinct turn ID. The final requests included both earlier exchanges.
