<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Reject changed seat bindings in account proofs

Base Codex source: `44481550d1bd2f889d69e651300b7647b9724af1`.
The audit and tests used fixtures only; no account registry, credentials, or
provider authentication were accessed.

## Reproduced problem

The probe joined retained account observations to the seats in its initial
`computers` reply. Its later machine-detail check verified the same live machine
key and label, but ignored the detail's seat bindings. If a seat moved between
those reads, observations from that seat could still count toward the original
machine. The old binding and later account observation need not have existed
together.

Using the existing test fixture, remove every preference and placement, retain
the two OpenAI observations on seat S1, and change the later machine detail to
`facts: []`. The old probe reports `matched`, `proven: true`, and two accounts for
m3. Both label and stable-key selection reproduce the problem. This is separate
from the earlier duplicate-label and changed-machine-lifecycle guards.

## Native schema and decision

Local aterm source `502447d29db289056d0bd576ae9c1ac6cebf9750` establishes the
relationship used by the check:

- `crates/aterm-db/src/alab/command/views.rs`, `seats`, derives each machine's
  `seats` list from `LabView.bound`.
- `crates/aterm-db/src/alab/command/verbs.rs`, `runs_on`, derives the detail's
  `facts` list from the same binding map and includes each bound row's `uuid`.
  `machine` supplies those rows from the same view and reports its completeness.

Require the later detail's unique, valid UUID set to equal the selected initial
seat set before joining account observations. Missing or malformed facts,
duplicate seats, and added, removed, or replaced bindings leave the probe
unavailable, with no proof or account report. Fact order does not matter, and
hardware fields are neither inspected nor retained. An empty seat set remains
valid when both replies agree; direct preferences and placements still count.

This detects a contradiction already present in the replies. It does not make
the separate registry reads atomic or prove provider authentication or quota.

## Validation

Before the production change, the new regression failed all six subcases
(three changed binding sets, each selected by label and stable key):

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_account_registry_probe.py -k test_changed_seat_bindings_cannot_join_retained_observations -v
```

After the change, all **16 focused tests passed** in 0.013 seconds:

```sh
/opt/homebrew/opt/python@3.14/bin/python3.14 -I -B -m unittest discover -s alab/tests -p test_account_registry_probe.py -v
git diff --check
```

The additional cases reject missing and malformed detail bindings and duplicate
initial/detail seats, preserve a stable multiple-seat join with reordered facts,
and retain a direct association for a machine without seats. Existing provider
filtering, account deduplication, observation redaction, machine ambiguity, and
private output checks remain in the same passing suite. No native build ran.

## Live read after review

The composed change also passed the same 16 tests. A subsequent read-only probe
of `mc-3212e085d1b70b59` with a minimum count of one returned `matched`, with the
label `m3`, one OpenAI account, and no errors. The initial seat list and later
detail facts passed the new consistency check. This reaffirms registry access
and the one-account association; it does not establish two accounts or fresh
provider authentication.

The private result is retained at
`$HOME/.aterm/.ubd/registry-seat-binding-live-20261112.json`, SHA-256
`c9338cf9461a742be1895dc0a59174c027eabab52d2d9c7f8d9119e0cfeed80e`.
No login, credential or assignment was changed.
