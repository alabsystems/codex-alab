<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Retain original-goal provenance while chronology is unreadable

A temporary journal-lock conflict was treated as a positive generation mismatch.
Live binding validation then retired a valid model stop; restart import also
closed its pending source. The journal can be locked without any goal mutation,
so later readability could not recover the evidence that had been discarded.

Decision: propagate unknown reads through the existing error/recheck result,
rather than invent a stale-generation observation. Source
`5c953cf68de6e0aaa866710ed87d5014a2204d3b` changes import and both standing
checks. Positive generation mismatch, native mutation epochs, person stops,
mode boundaries and final queue-admission checks remain authoritative. A newly
acknowledged duplicate stop still revokes its older source on uncertain proof;
that different mutation policy and its existing regression are unchanged.
No production journal guard is held through database awaits and no test seam
or public API is added. Only two ALab-owned files change, 247 additions and
13 deletions, formatted through source-selected Trust 15698. The entire prior
41,432-byte, 1,119-line test prefix remains byte-identical. Independent source
review `5f3f18a1…` found no blocker in the result flow or fixture ownership.

One maintained singleton `codex-alab-autopilot` invocation on exact 5c953cf6
passes ten distinct methods, zero failures or ignored methods, one library and
no sibling libraries. Two new controls hold the real stable chronology lock:
live binding/evaluation and a fresh Core/Harness restart retain the same source,
custody, generation, goal and request count while reads fail. Release without a
mutation imports the same source and produces one attributed continuation of
the original goal, preserving identity, objective, budget and used tokens.
Eight existing controls retain real host/person vetoes, durable generation
mismatch, uncertain restart history and intentional duplicate-stop revocation.
The actual V8 resolver, host artifact with both aliases, locked Cargo metadata
and native unit adapter are used; no helper is fabricated. These are Core and
loopback-provider fixtures, not certified CLI/SDK grants or whole-scene proof.
Manual due/evaluate steps do not independently prove a timer fire; the second
post-database read shares the result closure and is source-reviewed, not a
deterministically reproduced between-await race.

Original96766/24786→e4c69d closes0. Test, collector, coverage, source and guardian
statuses are zero. Complete before/after records match over 12,588 inputs and
eleven tools, SHA-256
`db497ad3e372dd83ba1cd9999bd07e78ebc6b80881a2ff0e68c51afbbd85898e`.
Raw unit log: `f145765a4db5274c278b7c6ca568d42d699984d0a0f2588842c56683036b7991`.
Independent closed audit `12814b8d…` reconciles exact methods, Source joins,
helpers, selected bindings and unchanged source/tool bookends. Deprecation,
linker and refused peak-record-write warnings remain recorded.

A fresh canonical executable gate on this new Rust production path,
original84662/41998→882a6b, closes1 at owned Tippy policy. It reports 940 source
warnings: 937 dead-code and three unused imports, zero source errors. Complete
normalized path/family/message and severity Counters against 3685c128 add and
remove zero source findings. The full inventory separately retains two tool
warnings and eight compiler note annotations; no format is unsupported.
Closed audit `b95831e2…` and log SHA-256
`6c06e8fb319bfaa854caf4dca38b4257d70f699f3e1130a71a31616bebd4c3b9`
retain that failure. The entire qualification identity is NOT stable: only the
observed main/base advances from d02ea891 to 36ecde11. Source 5c953cf6, tree
8ae90aee, integrated upstream ce303 and clean=true remain fixed. Qualification
is null, before certification, application runtime and later full suites.

Decision: land this tested, reachable recovery repair with the failed-gate
record. Retain the genuine unconsumed scene-integration backlog; do not suppress
warnings, widen APIs, fabricate Native grants or substitute an empty graph to
make the gate green. Source publication supplies no application certification
or installed adoption. Record this decision before a fresh authoritative read,
merge peer work normally and publish with an exact expected-main check.

## Merged main and final component checks

Fresh authoritative main `2231486202bfaf1b4f816d6ebcdc98022e840ca9` was merged
normally to `a718032d4b2bcb65e40cd4571fe95327dd0724eb`. Goal, chronology,
native-epoch and fixture sources remain byte-identical to tested 5c953cf6.
Incoming Core model-catalog startup/observer changes warranted one actual merged
rerun of the same ten controls. Loaded app-server refresh drops its shared permit,
awaits model work and repeats ordinary admission; that path is source-reviewed,
not claimed executed by this Autopilot library. Independent merge review
`5ca12083…` found no blocker or new direct goal/person-hold mutation.

Actual merged original61460/20839→4a0aa9 closes0: ten passed, zero failures or
ignored methods, one library and no siblings; all test, collector, coverage,
source and guardian statuses are zero. Complete before/after records match over
12592 inputs and eleven tools, SHA-256
`55063ff29290edd931a6e215a4ad004df25386dd34c3465c5676d49cc8febe2d`.
Raw log: `1545597a45b7e4d0643222d7f92ef7a24f5ebb8aed982cda3f4f2568b44c2781`.
The prior failed canonical gate remains tied to 5c953cf6 and does not qualify
this merge. Do not repeat an unchanged failed gate in place of these affected
component checks.

The updated fetch helper also passes all nine head-snapshot tests on exact a718:
concurrent transfers, whole-batch tracking updates, symbolic-ref guards, stale
cache refusal and authentication refusal. Source/test pins match before/after;
this uses local bare Git and mock typed errors, not real GitHub/SDK entitlement.
Log SHA-256: `464f42a69ebb0414093f4ad25dc7a99aa4400afd75f8bea8ff56e37af7670657`.
Decision: publish the tested source with a fresh read and literal expected-main
check, preserving peer work and the recorded certification limitation.
