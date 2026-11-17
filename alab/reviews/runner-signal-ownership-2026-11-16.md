<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Shared runner signal ownership

Decision: record each previous handler with `getsignal` before mutating it, including
SIGCHLD, and attempt every saved restoration independently. Reuse the standalone
`_successful_exit` classifier from metadata topic
`d0aff4786fc337bbe57b41dfe616b803da569615`; this topic adds no helper or import.

At base `89b3209b7324d314e44ca06f0f6b1ab97e5a0190`, a bounded public `run` control
performed each actual handler installation and immediately raised `KeyboardInterrupt`
or `SystemExit(0)`. All eight cases left the target handler changed because its old
value had not been stored. Separate actual-restoration controls combined an active
body error/cancellation with a restoration error: all 16 replaced the active primary,
12 skipped later handlers, and eight escaped as a zero-shaped exit. The test actor's
independent finally restored every real signal. No child was launched. The final
acquisition and active-restoration regression methods were also run against the old
runner after the metadata topic: their 32 subcontrols failed for these exact reasons,
without errors.

The runner now tracks its own active exception explicitly. It does not infer one from
`sys.exc_info` or an unrelated caller's handled exception. Signal-restoration failures
are collected after safe independent attempts, with nonthrowing advisory diagnostics.
An active operation error/cancellation keeps its exact identity. Without one, a genuine
nonzero exit or `KeyboardInterrupt` has authority over an ordinary or zero-shaped
restoration failure. Otherwise the first restoration failure is selected. A selected
success-shaped `SystemExit` becomes definite `RuntimeError` with its exact original
cause; ordinary and genuine nonzero/control failures keep their identity.

The child, guardian, Popen, deadline and evidence flow is unchanged. In particular,
existing child-cleanup exception precedence remains outside this fix: a child-cleanup
error still replaces an earlier body error as before, and signal restoration preserves
that selected error. This decision concerns handler acquisition/restoration only.

Validation: all eight new methods (53 bounded subcontrols) pass on managed Python 3.14
and macOS Python 3.9 with `-I -B`; the 57 existing methods in `test_capacity_exec` and
`weekly_reset_test` also pass without skips on both. New controls use real handler
mutations and independent actor cleanup, while Popen either refuses immediately or
returns an inert object and fixture receipt. They cover acquisition cancellation,
getsignal failure, every restoration position, exact-primary behavior, selected
zero/nonzero/control precedence, hostile diagnostics, and healthy/failed restoration
inside an unrelated caller's except block. `git diff --check` is clean. Independent
integration review approves ownership, local-primary selection and unchanged runner
scope. Independent hostile review approves and passed 29 bounded methods on each
interpreter without skips (new eight, metadata 16 and capacity five). Root reviewed
and approved the code and decision note.

Limits: a restoration attempt can itself fail, so the runner cannot promise the OS
accepted every restoration. It preserves that failure and still attempts all later
safe handlers. The new controls launch no application, native guardian, PTY, socket,
provider or compiler; no broader child/pipe cleanup claim is added.

Root composition `2c20c9ff4285c1131e202b7f62e992ef1c287636` includes the reviewed
metadata-reader topic and concurrent main's strict renewal-matrix status checks.
All 415 selected Python methods pass in 26.744 seconds, with zero failures,
errors or skips and the exact source clean and unchanged. The selection includes
four existing finite-process methods and nine runner-interaction methods, as well
as the new signal controls. Those existing methods exercise owned Python command
trees; they launch no Codex application or compiler. Only the two named pre-promote
fixture controls are selected, preserving native-required qualification boundaries.
Discovery alone finds 1,958 unique IDs, no discovery errors and all eight signal
identities, without executing any test body or native prerequisite.
