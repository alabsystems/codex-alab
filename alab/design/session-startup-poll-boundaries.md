<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Session startup producer boundaries

Startup stays on its caller's task. Synchronous non-inlined factories allocate
and erase inert async futures before those futures are polled. The delegated
async bodies retain their original validation, parent/source admission, history,
tracing context, cancellation and result propagation. No initialization is
spawned separately, and an unpolled producer performs no configuration or
credential I/O. Box allocation and inert future construction are synchronous.

`ThreadManager::start_thread` and `spawn_internal_session` delegate directly to
their original async inner methods through the existing ALab `boxed_startup`
factory. They introduce no extra async polling wrapper. Session's existing
phased startup separately returns preparation's continuation before polling
initialization; owned guards still span the same startup work.

## Diagnostic evidence and limits

The unchanged Core tree-capture regression on `09cbacd5ca` still overflowed its
ordinary test-thread stack. Root's LLDB R7 trace reached the initial fixture's
`Session::new`, not capture execution. The test poll's stack-pointer delta was
662,064 bytes (646.5 KiB); static disassembly confirmed its `0xa1000 + 0xa10`
reserve plus 32 bytes of saved registers. The trace also contained a separate
78 KiB `start_thread` poll frame constructing its boxed inner future.

Static inspection of that same `codex_core-fa7701d5be22c2f3` image found the
`spawn_internal_session` future discriminant at offset `0xf528` (at least
62,761 bytes of concrete state), embedded by the child helper whose own
future discriminant was at `0x13222`. In contrast, `prepare_exec_tree`,
`suspend_and_capture`, `update_thread_settings`, and `new_default_turn`
constructors wrote discriminants at `0x65b`, `0xa8`, `0x1a0`, and `0x4c0`.
Those small capture/default-turn producers were left unchanged.

The large startup producer is therefore a concrete caller-construction cost;
its share of the full 646.5 KiB test poll remains an inference. No fixture,
assertion, stack limit or auth gate was changed. The new producer boundaries
remain source-only until the original regression and compiled frame inspection
qualify their actual effect. This is not a passing startup or handoff receipt.

## Submission task wrapper boundaries

The next unchanged regression on `06d3c6629b` passed initial startup, then
overflowed inside the actual spawned submission task. LLDB R8 stopped in
`Instrument::instrument` constructing the concrete submission-loop future.
Its active callers included the submission body, `TaskLocalFuture::poll`, and
`AuthStorageOriginator::scope`, followed by the ordinary Tokio task harness.
The measured wrapper frames were approximately 438, 520, 345, and 693 KiB;
these are overlapping active stack costs, not a claimed exact future size.

The existing non-inlined allocation factory now erases the submission future
before tracing takes it, and erases the enclosing body before task-local scope
takes it. The outer factory still separates scope construction from startup.
There is still one original Tokio task, one original task-local scope and one
original tracing span. All asynchronous bodies remain lazy, and normal-loop
teardown, cancellation and owned guard destruction remain in that same task.
No fixture or stack setting changes. This additional boundary is source-only
until the original regression runs again; the trace does not prove its result.

## Dispatcher activity boundary

LLDB R9 on `596d7ef54c` showed the submission-task outer wrappers had
become small, but a later settings operation overflowed under the dispatcher.
The actual stack contained the dispatcher body (approximately 868 KiB),
concrete activity `scope` (456 KiB), and submission-loop poll (525 KiB).
The nested settings update was therefore reached with those active frames.

The owned activity-scope doorway now accepts an inert-future factory and
returns an erased future from a non-inlined synchronous producer. Its only
production caller supplies the original instrumented dispatcher expression
inside that factory. The concrete expression is constructed before the
activity wrapper and caller poll; both activity branches await the same
erased future. The match body, original span creation and instrumentation,
activity lifetime, same-task execution and cancellation remain unchanged.
No additional task, fixture change or stack-size setting is introduced.
This is source-only; the original regression must qualify the effect.
