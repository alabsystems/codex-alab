<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Request-recovery oracle hostile review

Review of landed `91cdeac0d6ac1b57dcdc3b4ea66b15e40e2a797f` found two
additional false-pass paths in the retained-work oracle:

- A trace containing an unrecoverable top-level `error` event could qualify as
  recovered if it also contained the expected answer and successful turn.
- An `update_plan` call in a different namespace could identify the fixture
  tool. Optional output name and namespace could also contradict that identity
  while retaining its call ID and result text.

Decision: reject top-level fatal errors in recovery and check the complete native
fixture identity. `codex-rs/exec/src/exec_events.rs` distinguishes fatal stream
errors from nonfatal error items. `codex-rs/protocol/src/tool_name.rs` treats
missing, null, empty and `functions` namespaces as default. Optional output
identity fields may be absent or null, or explicitly identify that same tool.
The checks preserve advisory warning items, expected fatal refusal diagnostics,
other history metadata, exact raw literals, IDs, ordering and retry input.
The CLI, cancellation handling and cleanup behavior are unchanged.

Twelve new methods exercise every fatal-event insertion position, valid warning
positions, refusal diagnostics, namespace/default forms, optional output identity,
input nonmutation and public case results with retained traces and cleanup.
They are included in the request suite's 61 methods, not counted separately.
The unchanged prior 49-method suite first passed on the landed source in
original run 4747/completion d6beca.

Test-only commit `a4277dccea707c0d5fc39dd44072c52361eb273a` retains the landed
production bytes. Original baseline 10061/completion a79ec1 exits 1: all 61
methods finish, 55 pass, four have failed subtests and two fail directly.
Twenty-four assertion failures expose false acceptance; there are no fixture
errors. The failing public namespace method stops at its first configuration;
the baseline does not claim all six public identity configurations ran.
One earlier invocation supplied a mistyped source ID and failed the observer's
HEAD precondition before packet creation or test discovery. Its separate log is
retained and excluded from the test results.

Production commit `563917d52b015e84ff83841c07760b0499339815` adds six lines and
removes two in the ALab-owned oracle. Original fixed run 9206/completion 1c8786
exits 0: the same 61 request methods plus five shared-capacity and sixteen
quota methods all pass. Both new test files, controller and executable maps
are unchanged; only `request_recovery.py` differs in the 871 captured input maps.

Concurrent main `0491766d84a53cf87e247ae9456eb81f06831cf4` changed the shared
smoke runners and smoke-test inventory. A normal merge produced
`5ea63ac9007651fe82ec7958ad0ddefcbc3a8ce1`; the oracle, its tests, shared CLI
policy and named-result collector retain the tested bytes. On that clean source,
original run 57853/completion 87f803 passes 96 methods: request 61, capacity 5,
quota 16, interaction ownership 9 and discovery 5. Original run 47200/completion
fde242 separately passes the 52 weekly methods through discovery in `alab/smoke`.
All 148 final method IDs are distinct, both original outer statuses and all six
maintained-runner children are 0, with no skips or errors. Each merged capture
retains 876 exact Git input bindings and three stable executable maps.

Original private result packets remain under `.ubd/codex-wide-composition-python-`
with these suffixes and exact index SHA256 values:

- `2gb8ggta` (prior 49): `101c916a5d7d50938a947c4d419cbca102dd9815f90186b264da20eb9262a389`.
- `i5mcrzdp` (baseline 61): `b15ebf6d364006c0c06efb2165b2e9554b322ca7f3b3717417c8920cfbe892bb`.
- `5y9n1yhm` (fixed 82): `a45bb0c288114290722ed0803d2e3bc50c55b53186a4b6558f7ddc32a6979f80`.
- `skmt42w2` (merged 96): `c143704cd19edeb5fed511d3e7fb972180d919981e2dbce43f7320c4ab1eae71`.
- `sqam39ji` (merged 52): `4a50d375f8d1f7db063877adf5d415dda098f9964de2b78e5e8ac06b960350f8`.

Source, tests, counters, original statuses, raw output, byte inventories and Git
bindings are independently reviewed. Controlled real children use managed Python
only; other executions use callbacks and inert fixture data. No native Codex
binary, provider account, reset credit, compiler, application gate or certification
is exercised. Existing human holds remain. No compatibility fallback or duplicate
runner is added, and no upstream-owned source is changed.
