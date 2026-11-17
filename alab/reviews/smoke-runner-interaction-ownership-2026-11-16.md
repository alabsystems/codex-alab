<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Smoke runner interaction ownership — 2026-11-16

Decision: close a guardian's owned pipes even when its interaction callback has
already reaped it, and preserve the original interaction failure or cancellation
if saving its secondary diagnostic fails. Reuse `capacity_exec.run` for the
guardian, signal restoration and raw-status checks instead of carrying another
copy in `weekly_reset`.

At source `433a952e2c`, direct public calls with finite inert Python children
prove both defects. A capacity interaction that calls `child.wait()` and then
raises leaves stdin and stdout open. A weekly interaction raises `SystemExit(143)`
after the child exits, but a collision at its diagnostic path replaces that
original cancellation with `FileExistsError`. The first five new test methods
fail four assertions and four error subcases against that source. Each child is
reaped and any leaked fixture pipe is closed by the test owner.

The weekly adapter retains its EOF stdin and allows its own cancellation
callback to settle the guarded command before recording the validated raw
status. Capacity interactions retain their writable stdin and existing immediate
cleanup when their callback raises. Both use the shared runner's unchanged
guardian evidence checks. The adapter saves diagnostic evidence in a `try` and
rethrows the original exception object in its `finally`; a secondary save error
remains available as exception context.

The shared deadline is snapshotted per call for guardian argv and validation.
Weekly passes its existing 120-second policy explicitly: `exec_accounts` sets
the capacity default to 230 seconds, and importing or changing that default
must not lengthen weekly reset checks. Controls verify both policies and a
callback changing the global after the guardian has started. No global is
temporarily reset by production code.

All nine new real-child methods pass on managed Python 3.14 with `-I -B` and
macOS Python 3.9 with `-B`. They verify exact exception objects, closed pipes,
owning reap, restored signal handlers, normal and negative raw statuses, stdin
policies, trailing output, diagnostic failure and refusal of changed guardian
evidence. Managed Python also passes the existing 20 capacity and 44 weekly
fixture methods (73 total); the 44 weekly methods pass on macOS Python too.
`git diff --check` passes. These are finite inert Python fixtures, with no
compiler, provider account, native application run or certificate claim.
