<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Share CLI completion policy without expanding execution authority

Exact source `2c14ca322044b15822eded161e0db8dcb988f520` still let the qualifier
and artifact-promotion CLIs report success after interrupted work or failed
diagnostics. Controlled original-entry child processes reproduce the failures;
no real qualifier, selector, build or certification gate is needed.

Decision: move the existing GitHub CLI classifier and generic diagnostic helpers
unchanged into stdlib-only `alab/cli_status.py`. All four function ASTs match the
previous definitions. Explicit adjacent `runpy` loaders avoid ambient module
search and avoid making qualification depend on GitHub transport. The reader
exports its three existing callable names for publication/version consumers.

Qualification now uses the shared classifier at both operation and reporting
SystemExit boundaries. Its custom primary/note headers, public `run` API and
gate command remain unchanged. Native POSIX wrapped integer exits such as
256/-256 become failures; float 0.0 retains its original exception identity and
already-failing process semantics. Integer classification remains bounded to
the interpreter's signed native-word range; overflow/noninteger payloads retain
the existing interpreter behavior.

Promotion uses the same classifier and protected reporter after argument
parsing. Normal selected updates and intentional returned no-ops stay successful.
Operation/reason-output interruptions and failing diagnostics cannot become
status zero. Ordinary failure output includes naturally retained cleanup notes.
Nonzero exits and KeyboardInterrupt still propagate. Selection admission,
conditional arguments, package validation and installed channels are unchanged.

The new helper is a runtime dependency, so the existing pre-promote source-input
comparison now names it. An owned minimal Git fixture with an inert version
printer proves that unchanged helper input is admitted, while a committed helper
substitution is refused before the printer is called. This exercises the source
comparison, not version validation or native certification. Fifteen physical
copy sites in fourteen existing fixture modules now include the helper too.

Focused coherent baseline/fixed comparisons use identical final tests and an
identical explicitly **new** helper input on both sides. The old programs ignore
that file; it is not claimed as an old Git blob:

- Eight qualification methods: baseline actual exit 1, seven assertion events
  across four failed methods and four passes, zero errors; fixed actual exit 0,
  eight passes. Each side records 23 real child cases executing both original
  top-level entry guards in order. Five observed false zero exits become 1;
  healthy/help zero, returned status 7, nonzero 47, KeyboardInterrupt and overflow
  controls retain their statuses. Float operation/reporting identity is retained.
- Ten promotion methods: baseline actual exit 1, ten assertion events plus
  thirteen error events across six failed methods; fixed actual exit 0, ten
  passes. Seventeen child cases per side change status counts from
  `{0:11, 1:3, 47:1, -2:1, 255:1}` to `{0:3, 1:11, 47:1, -2:1, 255:1}`.
  Eight false successes become failures, while selected/no-op/help remain zero.
  Natural notes come from real owned cleanup before a module-local fault.
- Two source-binding methods: baseline actual exit 1 with one intended
  substitution assertion and the unchanged positive control passing; fixed
  actual exit 0, both pass. An initial positive fixture incorrectly expected the
  hook's success line on stderr rather than stdout. Its failed records remain
  separate; only that test expectation changed before the final pair. A separate
  promotion packet preparation error occurred before any test and is retained.

Sources, tests, helpers and tool identities remain stable in the focused pairs.
Child witnesses distinguish called and returned operations and preserve original
entry AST identities. No exit wrapper or exception note is manufactured.

The final run uses clean committed merged source
`603ad49f5717c729c32621ba5dfec0a681e3181c`, tree
`1489d7ad081eee48c3b80bbf292160317d5bfd69`: 296 distinct methods, 294 passes,
two existing certification-dependent pre-promote skips, zero failures, errors,
expected failures or unexpected successes. Actual exit is 0; unittest reports
355.303 seconds. Nineteen full modules cover the Git, version, qualifier,
promotion and local-update behavior; nine additional positive fixture cases
cover source-copy callers. Focused pair counts overlap this run and are not added.

All 112 captured tracked ALab inputs/fixture modules, controller, tool entry
identities, Head/tree and clean state match before/after. Input closure and
execution scope were independently audited before launch. The run records
CPython 3.14.8 and Apple Git 2.54.0 on this POSIX Mac; wrapped-exit and signal
observations are not Windows validation claims.

Tests use owned Git, dummy gates, inert artifacts, fake selectors/registrars/
compilers and controlled Python child exits. The lint fixture requiring an actual
native Rust unit runner is not selected: its changed copy list is statically
audited, with no replacement adapter or fake passing result. Native producer/
certification work remains separately deferred. The two existing skip decorators
and the pre-promote certification note are unchanged. No production native gate,
Rust compilation, installed selection, entitlement or shipping certificate is
exercised. Incoming `0ddb254bd8d4985a948506797c65aa2a0090d2a3` was merged before
the check; its native autopilot test and review document are preserved, not
described as compiled by these Python checks.

The recursive common-Git archive is
`alab-validation/603ad49f5717c729c32621ba5dfec0a681e3181c/cli-status-0yk99662`:
510 payloads, 4,237,783 bytes and 208 immutable Git bindings. Root verifies exact
inventory, every payload size/hash and selected Git bindings, including the new
helper on both counterfactual sides. Raw pairs, child records, original proofs,
distinct preparation/fixture failures, complete root captures and controllers
are retained. Index SHA-256:
`9231f4c77333c685ace4ece1ea0b7a32c9db3d5bff91e6f0188232fc1c055412`.
Final root log SHA-256:
`f5809531a52f7b5068e072f7ff52cb7347c1deacfa0d953e7f6859b7c50bfb25`.

Publication also preserves remote main
`33eb7462bd464c4fd27587d09337cdf5113202e1`, adding native WebSocket diagnostics
and documentation. Of the 112 broadly captured inputs, only the non-runtime
`alab/REQUEST-DIAGNOSTICS.md` changed; none of the selected Python/shell inputs
reads that document. The other 111 inputs and controller remain identical.
The test result stays bound to `603ad49f5717c729c32621ba5dfec0a681e3181c`;
incoming native code is not claimed as executed, and a documentation-only input
change does not justify duplicating the functional run.
