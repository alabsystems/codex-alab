# Package-input fixture Git selection

The next local package check failed all three methods during fixture setup:
test_package_inputs.py hardcoded /usr/bin/git and supplied a system-only PATH.
On this Mac that executable is Apple's unavailable developer-tool launcher,
despite managed Git already being installed. The package behavior was never
reached; this was not an assembler or compiler failure.

Decision: discover the caller's Git with shutil.which, require it, and use the
same executable for fixture setup and its parent directory in the restricted
child PATH. This matches ordinary tool selection while preserving the private
HOME and system utility baseline. No product code, compiler policy, native
controller, installed package or global Git configuration changed.

The existing three methods now pass using managed Git. They check immutable
historical versus current target capabilities, rejection of incomplete or
unreadable revisions, actual assembly from inert prebuilt inputs, and the
negative control that an omitted executor input would invoke Cargo. No native
binary is built or executed. The baseline failed 3/3 before the change; the
fixed suite passed 3/3 in 3.207 seconds with the selected Git's bin first on
PATH, and passed again through the normal managed shim. Fixtures were retained.

Companion checks completed on this checkout: package library inspection 8/8
in 2.147 seconds; exclusive publication and target-cache boundaries 12/12 in
0.819 seconds. These use inert owned fixtures and do not select a real package.
Diff whitespace checks passed. No Rust check was needed for this fixture-only
change. The m157 rollout measurement remains deferred under the separately
recorded fleet ownership and label-binding refusal; no duplicate was started.

The follow-up scan found the same hardcoded launcher in source-version fixtures
and the canonical-driver executor containment test. Baseline source-version
setup failed all seven methods; the focused executor case errored before it
could enter its protected smoke. Source-version setup now uses the same selected
Git and restricted PATH pattern. Executor setup inherits the caller's normal
PATH already, so its Git invocation simply uses that selection.

Afterward, all seven source-version methods passed in 1.571 seconds, exercising
leaf stamping, historical source behavior, rejection before mutation on an
unreadable revision, and preservation/refusal of unexpected metadata or source
changes. All five executor smoke methods passed in 3.027 seconds, with no skips:
actual private child protocol/identity checks, malformed-response controls,
deadline kill/reap, and the actual macOS Seatbelt canonical-driver read boundary
(missing-read refusal followed by success with the explicit read). No permission
was broadened and no native build was run. No literal /usr/bin/git invocation
remains in alab/tests. Diff whitespace checks passed.
