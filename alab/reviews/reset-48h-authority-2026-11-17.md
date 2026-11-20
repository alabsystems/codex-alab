<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Restore the visible request's earned-reset threshold

The current conversation's human request says: “if there is more than 48 hours
remaining, to automatically use the reset (or else explain why not to use the
reset)”. Decision: restore automatic redemption only when the authenticated
ordinary weekly allowance refresh is strictly more than 48 hours away. Exactly
48 hours and nearer refreshes preserve earned credits and explain why. Credit
expiry is reported separately and still determines earliest-expiry selection.

Published commit `cad40d4ac03cd724f96ebf299e1a3f013eba7676` instead claimed an
explicit human request for more than 24 hours and a bounded provenance review;
merge `1cd3d088bb34b6d24c826bdad86b4a44222811e3` repeated that attribution. Those
commit messages cite no original message, thread identifier, quotation or
provenance artifact for the 24-hour request. Earlier 24-hour commit messages also
contain conflicting lower- and upper-bound interpretations. This bounded review
cannot verify those claims. It does not establish that no such instruction ever
existed elsewhere. The actual human text available in this session governs this
change; retaining the published 24-hour policy earlier was an AI decision, not a
new human instruction.

The production change in the reset crate is only the minimum remaining-time
constant, its adjacent comment and four policy-message strings. Keep finite
weekly utilization of at least 99 percent on the same ordinary window, verified
account and user identity, current-thread permission, final fresh usage reads,
expiry rechecks, idempotent operation keys and pending credit custody unchanged.
Polling, account handoff and notice delivery retain their existing behavior.

The existing pure boundary matrix now rejects 24/25 hours and 48 hours minus one
second/exactly 48 hours, and admits 48 hours plus one second/49 hours. Timestamp
extreme cases use the corresponding 172800/172801-second boundaries. HTTP
matrices retain real offered credits for negative cases and admit a 49-hour
positive control; the deterministic credit-listing clock starts at 48 hours plus
one second and tests remaining time above, exactly at and below the threshold.
Pending-key retry, inventory/expiry notices, deduplication, earliest-expiry
selection, confirmed restoration and account-change checks retain their actual
HTTP/ledger assertions. Preservation reporting also covers 24 and 25 hours.
Creditless list/poll fixtures move to 49 hours so their original purpose remains
eligible; malformed-usage reporting uses 72 hours so the invalid reading remains
the failure under examination. The feature invariant and scenario match 48 hours.
The autopilot side-wall integration fixture also moves from 25 to 49 hours,
with its absolute refresh and relative remaining seconds agreeing, to keep its
existing successful recovery assertions eligible under this policy.

Static verification used Python 3.14 to parse the feature TOML and compare its
structured leaves: only `/features/2/invariants/0` and
`/features/2/scenarios/21` changed. A complete pre-test production-slice comparison
against base `d8dd5e00e2b8930ee444b83d6ea994ff5d7ea34e` confirmed the limited
constant/comment/message edits described above. Scoped `git diff --check`
passed. An independent subagent reviewed all assigned crate and feature
diffs and found no issue. The initial system Python 3.9 static-check attempt
stopped before inspection because it has no `tomllib`; the managed Python 3.14
retry completed. These are source checks, not Rust test execution.

No Rust compiler, formatter, tests, tippy, certified gate, package build or live
reset redemption ran for this change. Root's separate native readiness probe
reports authenticated host resource custody unavailable (status 75), and qualified
publisher/unit-runner lookups remain unavailable. These boundaries are not
bypassed. The installed managed build `2026111701` identifies source
`70d63912591840da2a154f057dc0cade90a17d79`, which still contains the 24-hour
policy and packaged Sol 6.2 Ultra default. Root independently observed the live
executable/source identity and actual app-server config showing the user's Sol
6.1 Ultra override; those observations do not qualify this new 48-hour source or
ship the newer packaged default. A maintained qualified release and its native
runtime checks remain necessary. Prior nine notice and two forwarding tests
were inspected in source only, not newly executed by this work.

The maintained runtime matrix, weekly mock backend and notice oracle now agree
with this policy. Both preflight and recovery preserve offered credits at
24, 25 and exactly 48 hours; 49-hour controls spend and confirm restoration.
The existing 16-case runtime matrix keeps its explicit request-count and exit
expectations. Its negative fixtures still reject spending at 25/exactly 48 hours
and failure to spend at 49 hours. Notice checks retain exact backend inventory,
expiry matching, stage order, negated-action refusals and duplicate-observation
requirements. Current quota documentation describes the 48-hour policy; credit
expiry warnings retain their separate 24-hour window.

Managed Python 3.14 ran three updated methods against unchanged Python
production: ten assertion failures, zero errors/skips, actual exit 1. The final
candidate passed all 84 distinct methods across `weekly_reset_test`,
`test_reset_notice_oracle`, `test_check_runtime`, `test_reset_smoke_cli_status`
and `test_cli_status_binding`: zero failures, errors or skips in 6.239 seconds,
actual exit 0. HEAD remained
`c785f20eebec343a1d98f600a1e3816a8877c3b7`, and all 240 recorded ALab Python
source hashes matched before and after. Root independently checked the unique
IDs, result, log digest and final source bytes. Five edited Python files also
passed AST parsing. These tests use local fixtures and inert command dispatch;
they do not execute the reset Rust code or qualify an application package.

Evidence is under `~/.codex/.ubd/reset-48-oracle-20261117/`, including
`baseline.result.json`, `candidate-verified.{before,after,ids,result}.json`,
`candidate-verified.log` and `frozen-source.json`. The final log SHA-256 is
`098d15ebb36eaa902bdcb6d4c1b173fa79555264f6f2a8dd797e557ea892e27c`.
Two intermediate candidate runs exposed new fixture assertions that incorrectly
expected preflight/recovery usage to stay the same after seeding a wall. The
corrected table explicitly retains the real preflight 99-to-100 and recovery
0-to-99 observations. Those failed runs remain separate evidence; neither the
production backend nor its no-consumption checks was relaxed to pass them.
The baseline's `test_check_runtime.py` predates those fixture-assertion
corrections; the baseline and final run are not claimed to use identical test
bytes. Independent final review approved all 15 changed paths and verified the
84 actual passing log entries, unique IDs, 240-file bookends, policy boundaries
and separate credit-expiry behavior.
