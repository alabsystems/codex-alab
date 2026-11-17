<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# WebSocket rejection capture cancellation — 2026-11-16

## Decision

Close the cancellation runtime gap recorded in the preceding
[body-progress review](websocket-body-progress-2026-11-16.md). No new production
recovery defect was demonstrated. The new regressions check an existing ownership
contract: cancelling a pending handshake must retire its I/O without publishing a
completed rejection capture or a late diagnostic.

The audit also checked framing, replay, policy denial, retry-advice parsing and
opaque-error classification. Informational-response rejection matches the locked
upstream handshake; exact duplicate JSON header collapse is a documented boundary.
Neither observation was presented as a new ALab regression.

## Ownership boundaries

The component test uses the actual RejectionStream and Tungstenite client handshake
on a plain stream. An underlying Pending read follows ready response fragments,
proving the adapter processed those fragments. A healthy complete control first
checks original status, IDs, advice, body and complete byte counts. Four cancellation
rows cover empty or partial entities, before the deadline or after its timer is
ready while the owning future remains unpolled. Dropping that future destroys the
I/O owner exactly once. Shared capture state remains empty and tracked I/O poll
counts remain unchanged after advancing time beyond the capture budget.

The public API test observes a real loopback Upgrade request, delivered rejection
headers with a withheld entity remainder, and a pending owning call. It drops only
the connect future while retaining the client, factory and diagnostic subscriber.
The peer must retire within a bound, the server task must join, and no selected-target
completed or late diagnostic may appear after six virtual seconds. Real I/O cleanup
precedes pausing time. A healthy completed HTTP 400 validates the diagnostic path.
Two cancellation exchanges cover empty and opaque prefixes. Successful server writes
do not prove client decoder progress; that stronger inference is confined to the
scripted component test. These are eight operations within two new outer methods.

The initial public fixture required EOF exclusively. Review identified that a close
with pending or newly received data can reset TCP
([RFC 9293 section 3.6.1](https://www.rfc-editor.org/rfc/rfc9293.html#section-3.6.1)).
The corrected fixture accepts exactly EOF or ConnectionReset and rejects data,
other errors and timeout. It does not force or identify which close outcome occurs.

## Executed verification

| Source | Scope | Passed / failed | Scoped lint |
| --- | --- | --- | --- |
| `8fde1bd6fe64357b0aeccd4dd836028401495911` | Initial EOF-only fixture | 998 / 0 | Passed |
| `3b8c45fe8287e9b901567ad7f01f7c0452034e72` | Corrected peer-retirement fixture | 998 / 0 | Passed |
| `4904e369e1e903dc16a04814ab5700244ff822dd` | Incoming compiled defaults | 997 / 1 | Not reached |
| `6a6e413b5c99ba3fafeaa6841b8a8978f84ef7ad` | Updated defaults and Kimi test repair | 998 / 0 | Passed |
| `1b7c7f83e0fb5e79bfceff5d38a9d4262a2bb0c1` | Quota-source composition | 998 / 0 | Passed |
| `e8279c8dc87861eeca7f3173f7c0a0617c717b10` | Explicit Kimi High profiles and current peer tests | 998 / 0 | Passed |

Every run used the source-selected native Trust 15698 frontend, project admission
and explicit unverified test lane. All five complete libraries ran: public API 97,
ALab Core 424, API 281, HTTP 139 and WebSocket 57. Ignored, measured and filtered
counts were zero. Both cancellation methods passed in every run. The final run also
requires the current Kimi opt-in method exactly once. Final unit, lint and controller
statuses were zero; all-target scoped Tippy had zero owned, spanning, failing,
build-script and set-aside findings. No RED comparison is claimed.

Remote main advanced to `87023d47a720d440c07654e9bc6317f0924818bf`, selecting
Sol 6.1 and Ultra by default. These compiled inputs were preserved byte-for-byte,
so the native suite was rerun. The first composition exposed an obsolete Kimi test
assertion requiring no global default model. Both cancellation methods passed; that
composition is not a passing qualification and has no lint result.

The interim Kimi test repair checked no implicit global profile or provider,
retained exact K3/provider/API-key assertions, and checked that each explicit Kimi model differed
from the primary model. It permitted a primary default without coupling opt-in to
Sol's name. At that stage, effort-absence assertions described the profile
declarations, not effective inherited settings. Source review followed inherited
Ultra through the request
builder's model-owned resolution to bundled K3's supported High. No provider call
or unsupported literal-Ultra wire failure was demonstrated.

Publication preflight then observed main
`dc98611873ebad955ecb7f4aa87b28539a01f9fb`, containing 21 quota/notice,
smoke-fixture, inventory and documentation paths, including protected Core glue.
It stopped before merging or writing. The peer paths were preserved byte-for-byte;
all three own test modules stayed byte-identical. Native-five tests and scoped lint
were rerun on `1b7c7f83e0fb5e79bfceff5d38a9d4262a2bb0c1` with the passing
results above. The 23 weekly-reset Python fixture methods also passed on that exact
composed source. They are controlled fixtures, not application smoke, live credit
redemption, quota-actor/TUI Rust qualification or the full feature/application gate.

Main then advanced to `f810fd98e874d60aa1f11e5e496e013bdb36cf02`, adding
explicit High to both Kimi profiles and testing that their selected overlays replace
the base Sol/Ultra settings. This superseded the interim local Kimi assertion repair.
The merge kept the peer's entire Kimi test file and all six incoming paths
byte-for-byte, resolving the sole test conflict by retaining the peer profile
coverage. Own cancellation tests stayed unchanged. Fresh native-five
execution and scoped lint passed on `e8279c8dc87861eeca7f3173f7c0a0617c717b10`;
the controlled weekly-reset fixture tests were rerun on that exact source, all 23
passing. No resource-authority refusal was received by this prescribed-frontend run;
that observation does not establish repair of another worktree's authority.

Five live library binaries and the helper runner were independently bound to native
artifact digests, five-field snapshots, receipts and diagnostic identities before
each rebuild. Six separate observations retain the passing and failing phases.
Independent source, controller and native-artifact reviews are retained in the
session validation stage. The final candidate changes only this review after the
final runtime source. Own code changes are confined to the two cancellation test modules;
no production source, hook, dependency, manifest, lockfile or configuration is edited.

## Limits and deferrals

Tracked I/O and capture observations do not measure every internal timer or prove
all-system quiescence. These tests do not run a full Core/Autopilot recovery actor,
measure physical host sleep, establish backend acceptance, or diagnose the original
opaque HTTP 400 backend reason, which remains unknown. Recovery policy is unchanged.

An official release audit at 17:33 UTC found 24 private and 22 public Trust releases,
with newest numeric native package 27009 in both. The committed pin remains 15698;
its procedural-macro admission refusal and the unadopted 27009 build-script execution
refusal remain unavailable prerequisites. The audit did not rerun either compiler or
adoption. Another identical application gate was deferred. No application proof,
package, installation, binary adoption or managed delivery is claimed; existing
human holds remain in force.

The checkout and target stay frozen until native completion and live capture.
Interruption recovery requires complete native-job quiescence before source changes;
process-group disappearance alone is insufficient.
