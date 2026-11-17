<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Enforce the documented lifecycle before selecting evidence obligations

feature_evidence.snapshot did not enforce the documented lifecycle enum. It
skipped only the exact retired string, included other malformed values and
raised KeyError when lifecycle was missing. The fix refuses malformed values
before output, and all 87 portable feature test methods pass. The feature design
defines active, retiring and retired and explicitly includes active and retiring
contracts in snapshots; feature_inventory already enforces that enum.

Decision: require a string member of the existing three-value enum before the
retired omission. Invalid or missing values raise ValueError and the CLI refuses
before claiming a new output. Do not normalize casing or whitespace, or import
the complete inventory validator and its unrelated requirements. Valid active
and retiring contracts keep source order, literal IDs/descriptions and empty
pending checks. Retired entries still skip scenario access, and all-retired
catalogs keep the existing no-active-contract refusal.

The preceding landing inventory found all earlier packet heads published, no
active unfinished holder, only main and no stashes. Superseded reflog alternatives
were not reapplied. Only the two owned detached trees were aligned to
`2d76c43bb6af215c0a0eea8ab675edb3c7005dab`; shared root main at that commit
and pristine `ce3031018ef130e653ed4d9c5bb971652de97161` were preserved.
New topic work stays in a session-created worktree. The historical issue-read HTTP 403 was
not retried or interpreted as an absence of open issues.

The same eight new methods exercise 12 malformed variants through the public API
and actual isolated CLI: missing, unknown, empty, blank, padded, upper/mixed-case,
boolean, integer, float, list and table lifecycles. Controls cover output
device/inode/bytes preservation, mixed active/retiring literal order, malformed
retired scenarios, all-retired refusal, retiring-only inclusion/exclusivity and
both committed-catalog versus workcopy directions. The latter independently
subtests API and CLI refusal. Reused Git setup does not inherit another class's
methods; no runner mock or passing native adapter is introduced. Subtests are
not counted as separate methods.

The actual maintained baseline runner executes clean committed source
`c936a800e77a28fb0ff35a165a26d596b8d1deec`, whose production bytes match
the selected `2d76c43` baseline. Original controller 98868 exits 1. Eight distinct
methods finish: five pass and three have failed subtests, with 24 assertion
failure events and one error event. That error is the intended missing-lifecycle
API KeyError, not an import or fixture failure. Invalid baseline CLI cases return
0 and report pending-record creation. Their ephemeral JSON contents are not
retained artifacts; inclusion behavior is also established by source inspection.

The fixed run executes clean committed source
`21ad78a8a4282ad8efe686b1b6fe1c91880f6f45`. Original controller 87857 exits
0; the maintained runner reports all 87 distinct methods passed in 71.825
seconds, with no errors, skips, expected failures or unexpected successes.
Counts are custody 9, evidence 22, gate 12, inventory 27, scenario contract 9 and
lifecycle contract 8. The focused eight are inside 87 and are not added again.
These commands execute the maintained checkout; archived copies document its
inputs and are not described as private source-copy executions.

All 92 captured source inputs, including the design contract and fixture import
closure, match their exact Git commit and remain unchanged before/after each
run. Recorded tracked Git status is empty and Head is unchanged. The input maps
differ only in feature_evidence.py; the controller and new tests are identical.
The three captured Python, Git and /bin/bash executable maps match before/after
and across runs. Results record CPython 3.14.8 and Git 2.56.0 on this POSIX Mac.
This is the recorded tool scope, not a claim about every generated fixture
frontend or system utility. Existing gate consumers use inert hooks and an
argument-capture frontend. No native runner/compiler, actual gate/profile,
installed selector, application certification or held validation ran. Trust
adoption, protected producer and rejected release-validation work were not
retried.

A separate actual CLI probe at the fixed source exits 0, original completion
8a01b7. Its retained pending record exactly matches the actual committed catalog:
14 active features and 79 ordered literal scenario descriptions, all with empty
checks. That catalog contains only active entries; retiring behavior is covered
by the unit fixtures. These 79 pending declarations are separate from 87 unit
methods and qualify no scenario evidence.

The sealed common-Git archive is
`alab-validation/21ad78a8a4282ad8efe686b1b6fe1c91880f6f45/feature-lifecycle-contract-wj79_ow9`:
221 payloads, 4,388,707 bytes and 185 exact Git bindings. It preserves both
original unit indexes, raw outputs/results, before/after contexts, controllers,
outer receipts/logs, comparison, selection, pre-topic inventory and historical
permission record. The five real-probe files had no original index; their
post-run derived inventory is explicitly labeled and the original files are not
changed. Its catalog adds one Git binding to the 184 unit-input bindings.
Index SHA-256:
`64919956b7db218ecf06ccfd7bde2bcf9d72b0cc83694bd36dc913ea5ce8e47a`.
Original baseline index SHA-256:
`85bdd68298b8c14b0ab125f9d65fba31810a8f6f7fba97f7ca8b7d9611282dbd`.
Original fixed index SHA-256:
`33cc19e400175d3f1e8d57b43bf2465a40d7b8695340f6a4486f401abeb0cfec`.

Fixed feature_evidence.py SHA-256:
`1f7a361962ed5b0646a3d7c9810400e275f277426ebe9796dc380c7ee46af41f`.
Identical new test SHA-256:
`517ac6ac4688e284c1232eb3834b53cebf8e1632704399205c7068689b9eafee`.
Controller SHA-256:
`3ee136b56a4d758c0492319c64199fd5917b722512b79ea4120892a1ce678a28`.

Concurrent main advanced independently to
`cac62e661b8d7ce34a02a8079035f3f99f5371a2`. The ordinary merge
`444b801c4c380355811c9213e990f598d8585baa` preserves it in the owned topic
worktree. All 92 captured source/contract inputs remain byte-identical to the
tested fixed commit. Incoming changes are two review records, one intake JSON
and `alab-tui/glue/startup_account_read.rs`, outside the selected feature-tool
inputs. Their TUI runtime behavior is not claimed as executed here. The unchanged
selected inputs do not justify another identical functional run. The shared root
checkout was not moved by this session; its external advance is preserved.
