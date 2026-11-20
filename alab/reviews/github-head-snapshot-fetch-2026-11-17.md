<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Fetch immutable head snapshots through existing Agh admission

The requested cleanup comparison was rechecked against its actual starting main,
`bb7f29af32f578476d3628b012ebced49bd7d4d8`. All 83 sealed payloads passed hash
checks, the raw compiler counters still show twelve identities/24 events removed
with none added and zero errors, and all six published files exactly match the
compiled cleanup source. No regression or reason to repeat Rust compilation was
found. The existing source-specific comparison remains authoritative.

The next concrete gap was the branch race observed during publication. The
adapter enumerated main, then asked native Agh to fetch its mutable name with
the old expected object ID. A head advancing between those operations caused a
safe refusal and required a new snapshot. Native Agh already admits an exact
40-digit source when it equals the expected raw object ID.

Decision: fetch freshly enumerated `refs/heads/*` snapshots by that raw object
ID through the existing authenticated `--input` batch. Tag and other ref rows
keep their named expected-OID path, including the existing cold retag refusal.
An annotated tag is never replaced by its peeled commit. Native refusals are
neither caught nor retried, and no raw GitHub transport is introduced.

Heads sharing an object use one transfer into a canonical verified private slot.
An already cached desired head may supply the slot even when its destination
metadata is already current. Every selected slot is path/symlink guarded and
checked against the exact raw OID after transfer. Tag caches never supply head
groups. Cache names remain keyed by repository identity and ref, keeping bounded
metadata instead of allocating a persistent ref per commit. Each destination
retains its original old OID and fast-forward policy, with the same final atomic
no-deref compare-and-swap transaction. Explicit local fixture transport is unchanged.

Tests were pinned before production at `f2f167ae78`. The 27-method baseline
finished 21 passed and six failed, with no errors or skips. The real owned remote
advance reproduced the refusal after enumeration. The other failures exercised
the new protocol/cache expectations; they are not six distinct old integrity
bugs. Post-transfer mutation, existing tag-movement refusal, raw annotated-tag
identity and non-fast-forward controls already passed. The six new methods use
real private Git repositories and delegated transfers, not invented native
permission, source identity or successful replies.

Source fix `3fe8e27945` implements the reviewed head-only change. The updater's
inert Agh fixture now expects and verifies the exact source OID, retaining its
real Git object comparison and expected-OID checks. README documents branch
snapshot and cache behavior. New regression bodies remain unchanged from RED.

The broader 193-method consumer observation completed 190 passed, one failed
stale publication assertion and two pre-existing skips, with no errors. All six
new methods passed. The failed assertion still expected the main ref name in
the transport request; `558c812e4a` changes only that expectation to the actual
advertised main object, retaining complete-history and authoritative-main checks.
Both skipped methods carry the unchanged certified-publication hook note.
The ledger collector/guardian exit 1 remains retained: 48 ordinary ledger passes
and two skips do not constitute successful qualification of that whole selection.
The focused 15-method publication recheck on
`558c812e4acc5c9483fe787197c8ad5073bdd15c` passed every exact method, with
original process, guardian, collector and source statuses zero and no failure,
error or skip. Its only changed input from the broader run was the assertion
file; the production helper's Git object was identical. The sole failed broad
consumer identity now has a passing observation. These are overlapping methods,
not 15 additional distinct tests or a new all-193 qualification result.

The focused summary SHA is
`f92e8076268a07274a5e9cec542ebd2411debc9fd3e4082b0a0cd2ab859f6fbd`,
under `codex-github-head-snapshot-push-green-558c812e-0vupm39x.noindex`.
The combined finite packet is `codex-github-head-snapshot-evidence-op4horvb.noindex`,
index SHA `10578916b643f5fd48c9a1e6d44e3862f66cfce3abf939b44b10eef626d99643`.
It preserves all original results, exact methods, raw outputs, statuses and
unchanged source/tool bookends for each run. The ledger refusal and both skip
reasons remain recorded. No full source trees or application binaries are copied.

An additional authenticated read used the actual installed Agh client and the
new helper, importing the enumerated main object into an owned bare repository.
The actual row's source and expected OID were identical, and both private and
origin refs matched the observed snapshot. Guardian/process exit was zero and
the caller's worktree stayed unchanged. Local Git object data was prewarmed as
data only; native admission still ran. The owned bare data was removed after
retaining its refs, request, response and summary. This proves installed-protocol
compatibility, not a live induced server race or a Native source-exec transition.
The controlled remote-movement test supplies the race schedule.
Live summary SHA:
`a05e9597579400cf622ff6ef3d4c104b664a4ea073898b1f5477824447d0c332`.

RED packet: `codex-github-head-snapshot-red-f2f167ae-d3i8hrm6.noindex`,
summary SHA `0c63532af0afdbee1e8bd056edf805cd1d5ad6df3a29c7c797d08ebabc21ae19`.
Broad consumer packet: `codex-github-head-snapshot-green-3fe8e279-rsyd8gjn.noindex`,
summary SHA `fbf64d1e4f7ffe00a7049816878745f096b6582df4a38563ae7f820f3ddb6abf`.
Live read packet: `codex-head-snapshot-live-4d6nbfa_.noindex`.
Independent design/source review approved the cache grouping, guard placement
and unchanged destination transaction. No application build, certification,
package promotion, installed-app change or new native runtime authority is claimed.
