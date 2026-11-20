<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Keep a failed goal resume's retry with its native point

A failed native resume can finish reporting after its runtime has been
replaced. Schedule its retry through the existing registration and decision
checks, with the same point, policy and Off epoch. A recheck observes quota
afresh, so an allowance that was withdrawn or aged out during reporting cannot
strand a still-current point. Final dispatch still requires fresh authority.
Independent review confirmed both the registration and quota guards.

Seven focused tests passed on merged revision `323bdd0579d9e3cab1815dac4f6f4a3517b75c6a` with Trust 15698.
The native fixture forces a goal write failure and holds the actual fake-host
attention request. A replacement registration with the old stop flag unset
leaves both timers empty. An unchanged registration queues one delayed retry.
A third case invalidates an allowance for the same account while reporting is
held. It drives the normal timer with the simulated host sleep reading and
observes the adjusted usage reset, with the original goal and zero model
requests. This targets loss of the allowance observation without waiting for a
real age-out or restarting Core.

An earlier scoped run was stopped after review identified a reset comparison
that omitted the simulated sleep. The final predicate accounts for that sleep.
The maintained runner, collector, coverage observer, source comparison and
command guardian returned zero. All 12,621 source inputs and tool records
match. Verification covered focused Autopilot tests.

- Unit log SHA-256 `c4a3cf39fd3534f97ce20ad88250bc74e3daa818638923479139aa17e61eb2b9`
- Source record SHA-256 `f25e326844760dc2b27acd011f3da631c39c26c19287a5aaf7b7217bb3959542`
