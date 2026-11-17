<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Refuse incomplete updater and collector CLI operations

Baseline `673c454e5d61f6f574a598103b08dde79cb183ff` has two observed false-success paths.
Original updater entry points can exit zero after an operation raises a successful `SystemExit`,
or while failure diagnostics abort. The collector naturally raises `Refusal(status=256)` or 512
for an interrupted process; POSIX truncates either process exit to zero. These paths do not
establish completed work.

Decision: reuse the existing stdlib-only CLI policy at the operation boundaries. Updater `main`
guards post-parse operations and diagnostics; its source-owned guardian inline program calls a
thin `run_fixed_cli` adapter. The private adapter delegates directly to the same-source
`run_fixed_locked` entry. `run_locked`, `run_fixed_locked`, fixed-source admission, custody
arguments, locks and journal completion bodies retain their baseline ASTs. Interrupted APIs keep
their original exception objects and incomplete journal observations. The adapter does not
re-enter public dispatch or reacquire the guardian lock.

Collector `Refusal` now accepts only exact integer error statuses from 1 through 255; other
values become 1. This keeps valid interruption statuses such as 143 and 255 while preventing
wraparound success. The post-parse collector CLI guards successful `SystemExit` and protects its
error reporter, then returns the validated refusal status. `prepare` and `collect` retain their
baseline ASTs. Parser help, successful returned operations, nonzero cancellation and
KeyboardInterrupt retain their existing behavior. The shared helper is unchanged, already
source-bound and already copied at the collector's single physical copy fixture.

Identical final tests run against coherent exact-baseline/fixed source bundles:

- Updater: baseline actual exit 1, nine methods, 21 assertion events across four failed methods
  and five passes; fixed actual exit 0, all nine pass. Each arm runs 36 original-entry children.
  Sixteen observed false successes become failure; healthy/help, returned 7, cache-busy 75,
  nonzero 47, KeyboardInterrupt, float and native-word overflow controls retain their statuses.
  Direct APIs preserve cancellation identity and only incomplete journal observations. The
  extracted original guardian inline program uses controlled admission, proving argument
  forwarding rather than real lock/admission behavior.
- Collector: baseline actual exit 1, ten completed methods with 20 assertion and 11 error events
  across six failed methods; fixed actual exit 0, all ten pass. Each arm runs seven natural CLI
  cases and thirteen controlled original-entry cases. Nine false successes become failure,
  including real minimal-state status 256/512. Interruption 143/255, rejected
  negative/unprepared state, API cancellation, help and controlled dispatch controls remain
  valid. A nonexistent private script invocation failed before any test; its record is retained
  separately from the first actual paired run.

The final broader run uses clean committed source `334bcfc0824c230ac555b96810132a5070ef2571`,
tree `2865e4da23e38bfbd41a1737271b0c376a56ae67`: actual exit 0, 145 distinct methods all
passing, no failures, errors, skips, expected failures or unexpected successes. Unittest reports
29.515 seconds. Nine full modules cover updater behavior, collector directory/prepare/collect
cleanup and observation checks, shared CLI policy and both new modules. Three existing guardian
integration cases exercise source custody and locks against owned Git, inert builders and fake
selectors. Focused pair counts overlap this run and are not added.

All 97 captured tracked inputs, controller, tool entry identities, Head/tree and clean state
match before/after. The input closure and bounded execution scope received independent review
before launch. Results record CPython 3.14.8 and Apple Git 2.54.0 on this POSIX Mac. Native
runner/CheckLint suites are not selected; the collector's single physical copy list is a static
input and already includes the unchanged helper. No actual native runner, Rust compiler,
production gate, installed updater/selector, LaunchAgent, entitlement or certification is
exercised. Prior native certification and producer-channel work remains separately deferred;
these Python checks make no application certification claim.

The final recursive common-Git archive is
`alab-validation/334bcfc0824c230ac555b96810132a5070ef2571/updater-collector-cli-7ky_czj3`: 563
payloads, 4,114,383 bytes and 186 immutable Git bindings. It retains both original proofs, both
paired runs, raw child observations, all root captures/logs/controllers and the private
preflight failure. The collector packet also carries a duplicate initial proof; the final
inventory explicitly binds those two copied source files too. The first archive candidate/config
remains separate packaging history, with no code change or functional test rerun. Root verifies
exact inventory, every payload size/hash and every declared Git source binding. Index SHA-256:
`0f19d6fd46b3c72aec9365f23db6106cd70eec95ad1172cc3e3878eb82278c3b`. Root log SHA-256:
`226f15ec79e9c2c58dddb14c0520313fbe5de11a5bade097f6fca7fd54babcbb`.

Publication also preserves concurrent main
`d5957cf569695d2f07c738cf6cda910e569504d1`, which adds native goal mutation
custody work and documentation, through merge
`34196e320dfb658ad39f1ede9b3e1ee057e22fd9`. Among the 97 broadly captured
inputs, only the non-runtime `alab/CONTRACT.md` changed. The other 96 inputs
and controller remain identical, and the selected Python/shell code does not
read that document. The test result stays bound to `334bcfc0824c230ac555b96810132a5070ef2571`;
incoming native code is not claimed as executed. This document-only change to
the captured inputs does not justify duplicating the functional run.
