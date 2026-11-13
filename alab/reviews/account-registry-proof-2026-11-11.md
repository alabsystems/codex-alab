<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Native account registry inspection on m3

The requested proof had two parts: codex-alab can read aterm's account registry,
and two OpenAI accounts are associated with m3. Live inspection on November 11,
2026 proved the first part and found only one account for the second. Do not
turn the requested count into a claim about the data.

## Observed behavior

Opening `/accounts` in the running codex-alab TUI returned a native account view:

```text
Subscription accounts · cached native observations
Current session account: unavailable
Session selection mode: unavailable

codex-1cde97 · a***@gmail.com
unseen · enrolled · stale usage
```

The same label appeared in `aterm harness accounts list --cli codex --json`.
The read-only registry commands `aterm db computers`, `aterm db ai-accounts`,
`aterm db alab preferences`, and the `account-obs` table scan all succeeded.
The native view proves codex-alab's authenticated host connection; the shell
commands separately prove access to the local replicated registry. Neither
proves fresh provider authentication, available quota, or account-switching
authority.

The account view deliberately leaves the assignment unknown. In aterm's
`crates/aterm-gui/src/codex_accounts_control.rs`, `snapshot_in` emits a null
assignment until the native slot identity and generation can establish it.
The known email in the Codex footer is a separate loaded-auth observation.

## Machine identity and count

The registry returned two live machine rows named `m3`:

| Stable machine key | Binding | OpenAI associations found |
| --- | --- | --- |
| `mc-3212e085d1b70b59` | Node `a20d9412c90c0be0`, seat `9bb31b76808c88373b5f21647e7cfe4f` | `ac-1cde978e0de57819` (`codex-1cde97`) |
| `mc-ce20a1499bba52af` | Imported row, no node or seat | None |

The current machine's account observation names the OpenAI account above.
Its native placement view names the same account; its 14-day journal also
contains only that OpenAI identity. The imported machine's preferences are
three Claude accounts and one agy account. The registry's complete-view flags
describe the local retained view, not a fresh response from every fleet peer.

`aterm db alab machines m3` refuses the ambiguous name. Resolving each stable
key explicitly works. `aterm db alab merge --dry-run` proposes folding the
imported row into the node-bound row, carrying those four non-OpenAI
preferences. It would not establish a second OpenAI account. The broader
merge plan includes unrelated machines and was not applied.

## Decision

Keep registry identities, account assignments, and credentials unchanged.
There is no evidence here for inventing another account, importing a sheet,
starting enrollment, or applying a fleet-wide merge. The duplicate label is
an identity ambiguity to report explicitly, not permission to combine
different machines' account counts.

Retain a read-only diagnostic in `alab/account_registry_probe.py` so another
agent can repeat the registry query, select the stable machine key, and
check the requested count. Its output retains metadata only. It does not
read credential files, contact OpenAI, refresh authentication, or exercise
the TUI connection; the native `/accounts` observation above is separate.

The original private evidence is under
`$HOME/.aterm/.ubd/codex-m3-registry-proof-20261111.noindex/`: `proof.json`
and `native-accounts-view.txt`. That first ad hoc JSON covered preferences
and account observations; the repeatable diagnostic additionally inspects
native placements and rejects ambiguous success.

Repeat the inspection from the repository root:

```sh
python3 alab/account_registry_probe.py --machine m3 --expect-at-least 2
python3 alab/account_registry_probe.py --machine mc-3212e085d1b70b59 --expect-at-least 2
```

Exit zero means the selected machine has at least the requested number of
distinct OpenAI account associations. A missing machine or insufficient count
exits 1; ambiguity, unavailable metadata, and an evidence-output error exit 2.
`--output` creates a new JSON file with mode `0600` and refuses to overwrite an
existing file. The command always reports that local registry reads are not
fresh provider authentication or an atomic snapshot of the fleet.

## Validation

All ten focused tests passed with:

```sh
python3 -I -m unittest discover -s alab/tests -p 'test_account_registry_probe.py' -v
```

The tests cover duplicate labels, account deduplication across association
sources, other providers and retired rows, incomplete/conflicted data,
dangling references, malformed observations, native unbound-seat rows,
query errors, output redaction, and private output files without overwrite.

Three live runs verified the CLI result and exit status together:

| Selector | Requested minimum | Result | Exit |
| --- | --- | --- | --- |
| `m3` | 2 | Ambiguous; separate machine counts of 1 and 0 | 2 |
| `mc-3212e085d1b70b59` | 2 | Insufficient; found 1 | 1 |
| `mc-3212e085d1b70b59` | 1 | Matched; found 1 | 0 |

Their metadata-only output files are `probe-named.json`, `probe-key-two.json`,
and `probe-key-one.json` beside the original private evidence. These results
verify the diagnostic and the observed registry state. They do not satisfy
the original request to identify two accounts.

## Additional account in the shared registry

The subsequent request was to find another account anywhere in the registry,
without requiring an association with m3. This succeeded:

| Field | Observed value |
| --- | --- |
| Account | `codex-0e3401` (`ac-0e340165827982db`) |
| Provider / service | `openai` / `codex` |
| Registry lifecycle / status | `live` / `active` |
| Login address reference | `ad-9f8c8db379bdec05` |
| Machine | `m68` (`mc-b827f3ab169c826f`) |
| Retained login observation | `idle`, `codex`, `auth-file` |
| Observation time | November 12, 2026, 05:37:11 UTC |

The canonical account and address `table get` commands resolved the identity.
The `account-obs` scan joined its seat to m68 in `aterm db computers`.
`aterm db alab who codex-0e3401` showed no current claim or holding;
`aterm db alab quota codex-0e3401` had no window readings. These establish a
different registry account and an observed login, not present usability or
available quota on m3. No sign-in, preference edit, or assignment took place.

The follow-up exposed a diagnostic gap: its `retained-observation` source tag
discarded the observation's time and state. Preserve those fields alongside
each account's association evidence, including a `signed-out` observation.
The association count stays the same; the report must not promote any of
these observations into a successful authentication claim. Account emails,
credentials, and free-form source fields remain outside the saved report.

All twelve focused tests passed after adding observation evidence. The two
new cases preserve old signed-out and newer idle/running records, reject a
foreign seat's evidence from the selected machine, deduplicate exact entries,
and leave the evidence empty when only a preference exists. A live run for
`mc-b827f3ab169c826f` matched `codex-0e3401` and retained its exact idle state
and `1794461831000` timestamp in milliseconds. Its report is
`probe-m68-observations.json` beside the earlier evidence; the independent
account lookup is recorded in `other-account-proof.json`.

## Contradictory machine reads

A follow-up review reproduced a false success when the initial machine list
named a live `m3`, but the later detail read returned the same stable key as
retired or renamed. The original diagnostic checked the key and completeness
but ignored the later lifecycle and label, so it still reported `matched`.
This is reachable: aterm's stable-key lookup returns retired rows for history.

Reject a detailed reply unless its machine is still live and its label agrees
with the initial selection. A detected rename or retirement makes the result
unavailable, with no account-count proof. This does not make separate registry
reads atomic; it prevents a contradiction already present in the replies from
being ignored. The change affects the diagnostic only and writes no registry
data.

All thirteen focused tests passed. The new regression exercises retirement,
merge, rename, missing fields, and a different stable key through both the
analysis function and the CLI report builder; each returns no proof. The
three native checks retain their expected results: ambiguous `m3`, one
account on its node-bound record, and the successful one-account check on
m68. No live registry mutation was needed to reproduce the defect.
