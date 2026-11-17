<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Deferred inherited display history

The retained handoff consumer now obtains a deferred child's complete display prefix from the original frozen lineage plus its actual local writer cutoff. It no longer rejects every inherited history_base. The exact acquired writer identity still fences discard/recreate ABA. The original local-only snapshot contract is unchanged, and metadata-only projections still use that local-only read rather than requiring ancestor IO.

The distinct complete display API seeds the original RolloutLineage resolver with the writer header's actual HistoryPosition. The parent's current rollout is not substituted after a revert. The same seed survives the resolver's existing alternate-copy fallback, and every read attempt reapplies its exact cutoff. Reads use original tracked ancestors, compressed readers and attached-history display policy; contiguous raw record ordinals are checked before combining inherited records and the local cutoff. There is no model-context filtering, synthetic header, force-persistence or empty-success fallback. Observation cancellation reaches the blocking reader; errors remain errors.

The existing listener, actual Core/PrimaryLease/currentness checks, initialized-recipient subscription, original settings/account/replay ordering, separate Native RuntimeCommit and same-Hold release path remain. Requested history combines only a captured complete inherited prefix with the actual local items. An absent requested prefix is an error. Other materialized/ephemeral paths keep their original readers. Ordinary activation remains disabled pending the full UI/selected/NoPrimary/editor and signed-image handoff.

## Validation

Source-selected Trust 15698 and the unchanged protected profile run component tests; no application certificate or Native SDK/UI Go is claimed.

- Full thread store: 305 pass, 0 fail/ignored. Five new actual-store controls cover original fork cutoff vs later parent append, compression/corruption without materializing a child, writer generation and materialized None, actual parent revert, alternate-copy fallback after a bad first compression read, and original attached-history visibility.
- Actual Core bridge: 3 pass. New typed-fork control captures a real fresh deferred root, one gated model turn, original Standard-materialized typed fork, deferred APIs returning None after materialization, and original typed inherited turn/assistant text with no extra model request. The old Core/generation and real-name materialization controls also pass.
- Full client library: 61 pass.
- Full app-server library: 618 pass, 0 fail/ignored.
- Final Tippy on thread-store/Core/app-server/client libraries and tests: owned/spanning/failing/build-script findings all zero; 12 preserved upstream diagnostics set aside. Their primary sites are outside added hunks and match the committed base. All four selected packages have no build script.

The original typed fork deliberately materializes persistent children. The new control verifies that rule; it does not fabricate a deferred fork Core or weaken persistence. True deferred inherited children are exercised through the actual lower store/fork reservation operations. The first store run304pass/1fail exposed a missing fixture reconciliation before unloaded revert; it now uses the original reconcile_rollout on the actual writer file, as upstream's revert tests do. The first Core compile exposed ThreadId lacking FromStr; the test uses its existing explicit parser. Failed logs/inputs are retained.

The complete goal still requires current-primary/NoPrimary and Source transition ownership, full original UI startup and pending/editor/input transfer, the actual Native Source prepare/enter and S1/S2 pair, signed-package G1/SETEXEC/E1/UI qualification, and the certified application gate/main landing. These component results do not establish any of those remaining outcomes.

Final checked code input: base `790777104e3c8ed4e1bb4e2e842b01d91511d5ce`, patch SHA256 `f429c47a3799a0629aaade3bed2ac434760e3ff892cd74e8b0e98c1a82e2abc7`, 14 paths. Runtime input was `21624e68b29689fca2165c531b899c32cff11f1f5ed609ae076a9ccc29f8bf1f`; the sole final change collapses an equivalent nested metadata check into an if-let chain, with production/tests recompiled by Tippy. All store code matches its passing input. Across store/client/server libraries there are 984 distinct passing cases, 987 executions because the three Core cases repeat in the complete server suite. All six new named controls pass. Compiler/dependency locks are unchanged. These are unverified component executions, not certified application builds.
