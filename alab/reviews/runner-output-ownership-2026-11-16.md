<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve shared-runner outcomes through output close

Decision: replace the runner's nested output-file contexts with a small private
owner. Store each returned exclusive file before the next acquisition or body,
close both independently and retain the exception already selected by acquisition,
the body, child cleanup or signal restoration. This adds no runtime input edge.

At `546e672356`, actual-close-before-error wrappers around owned output files
showed that close failures replaced active operation/cancellation outcomes,
including the error selected by child cleanup or signal restoration. Failure to
open stderr was likewise hidden by stdout close failure. A healthy close-only
SystemExit(0) escaped as a zero-shaped exit. The final nine-method regression
module, run against the unchanged baseline module, produced 45 expected failing
subtests without errors. Positive visible post-open cancellation, exclusive-file
collision and normal output controls already passed; no additional leak at those
visible returned-line boundaries is claimed.

`_OutputLogs` stores stdout and stderr directly inside its acquisition guard and
passes those same objects to the original runner body. It retains exclusive `xb`
opens, paths, names and output behavior. Closing retains the previous stderr then
stdout order, but every owned stream gets its independent attempt. Active errors
keep their exact identity; close diagnostics use the existing nonthrowing labels
and note transport. With no active error, close failure prevents success using
the existing ordinary/genuine-control selection and zero-exit normalization with
the exact original cause.

The owner is a class, not a generator context manager. Its `__exit__` returns
False after attaching secondary diagnostics, letting the with statement retain
the active object. A generator owner would expose PEP 479 StopIteration
conversion and contextlib's traceback attribute assignment, which can replace the
existing hostile-primary control. Explicit ordinary/active StopIteration and
unavailable exception-attribute/formatting hooks are covered by the new tests.

The production runner changes only its outer with statement. Its command,
deadline, body, child-cleanup, independent signal-restoration and guardian-receipt
logic remain unchanged. The output owner closes actual returned file objects;
it neither invokes their custom context entry/exit nor introduces raw-FD,
background IO or guardian authority machinery.

Nine new methods cover 64 finite controls with real owned file wrappers and
inert or refused Popen boundaries: body/child/signal selected errors, both close
faults, first/second acquisition refusal, multiple-close control selection,
StopIteration, hostile diagnostics, visible post-open cancellation, no-overwrite
and original handle/name/output/raw-status behavior. Faults occur in actual close
after the owned underlying file is closed, not solely in wrapper `__exit__`.
Fixture fallback uses the underlying handles, then independent counters require
exactly one close attempt for each returned output and no unowned closes.
Forbidden child/raw-signal mocks are independently asserted unused.

All 48 selected methods pass without skips under managed Python 3.14 and system
Python 3.9 with `-I -B`: nine output-owner methods, ten child-cleanup methods,
eight signal-ownership methods, five capacity methods and sixteen package-reader
methods. `git diff --check` passes. New controls launch no child process,
application, native guardian, socket, PTY, provider or compiler.

Limits: a close attempt that fails is evidence of incomplete output cleanup, not
proof the OS closed the file. The fault fixtures perform the real close before
injecting their error and distinguish that observation from fallback. The owner
cannot recover a resource an open operation conceals without returning it.
This changes Python ownership and failure policy, not native retirement,
application qualification or build certification.

Both independent reviewers passed all 48 selected methods on both Python
versions and approved the frozen source, controls and stated limits. Root read
the class, runner diff and note and approved these three paths for composition.

Root's final composition at `1d888ea7f7c22ccddfa214b08aa346c42647f57a`
preserved concurrent main changes through `3a4846e3a5`. All 434 selected Python
methods passed in 28.489 seconds without skips, failures or errors; HEAD and
the worktree stayed unchanged. This includes the nine output methods, ten child
methods, eight signal methods, four finite capacity-process methods, nine runner
interaction methods and two explicitly selected pre-promote fixtures. Separate
discovery found 2,012 unique test identities with no discovery errors; discovery
does not run test bodies or admit native prerequisites.

The incoming renewal-matrix change binds parsed results and their digest to the
same observed bytes. Its seven new result-binding methods separately passed in
0.054 seconds on that composition. The unrelated incoming request-timeline code
was preserved; it is not part of the selected 434-method run. These checks do
not replace the full native-gated Python workflow, remaining Rust reset tests,
the certified application gate or package adoption.
