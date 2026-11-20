# Native codex-alab release controller

`codex-release prepare --repo PATH --source FULL_SHA --intent PATH` freezes
the source-owned `MAJOR.MINOR.0` product version, upstream client version and baseline,
target, compiler lock and dependency lock. GitHub access goes through Agh.
The intent binds the canonical GitHub repository derived from its admitted origin;
changing that origin invalidates it. Publication ownership remains atpkg's configured
authority. The controller creates each immutable record once. On retry it checks and flushes
the existing file and its parent directory. An exact replay needs no temporary
file, and conflicting inputs leave the original record in place.
`codex-release status --intent PATH` reads it back.
The baseline is the one the committed version ledger row records, and the client version
must be above that baseline's line. No upstream ref is read: a stable tag that appears
after the version was claimed changes neither the intent nor what the source releases as.
Intent files are private controller state, not package payload or public release metadata.

`compile --repo PATH --intent PATH --publisher PATH --cargo-home PATH
--read INPUT --write OUTPUT -- TARGO_ARGS...` rechecks the intent and delegates
compiler selection to `atpkg-publish toolchain run --source FULL_SHA`. Its worker
uses native explicit-target admission inside a protected process with declared
read and write roots and a scrubbed environment. The parent requires a worker
completion record bound to its intent, argument bytes and private operation nonce;
a controller returning zero without running the worker is insufficient.
For direct `build`, `check`, `test`, `run`, `bench`, `doc`, `fix`, `rustc`,
`rustdoc`, `install`, `package`, `publish`, `fetch` and `vendor` commands,
the public controller checks each declared writable directory, the private Cargo
home, common Git directory and compiler scratch volume for 20 GiB before the
source fetch or selector starts. This also covers a leading `--unverified` and
the `trust build`, `trust check` and `trust test` forms. A shortage exits 75.
Direct `--version`, `--help`, a command followed immediately by `--help` or
`-h`, `install --list`, `package --list` and `package -l` remain available
below the floor. Flags passed to the built program do not bypass the check.
Cleanup, project aliases, other external commands and invocations with additional
global options before the command retain the frontend's admission.
Full `metadata` resolution checks the private Cargo home, common Git directory
and compiler scratch volume. It can unpack cached dependencies even with
`--offline` or `--frozen`. The same check applies when `--locked`, `--offline`
or `--frozen` precede `metadata`. Workspace-only `metadata --no-deps` stays
available below the floor.
The compiler worker and its native selector preserve resource status 75 and
report an interruption as 128 plus the signal number. Other compiler failures
exit 1. Only a successful stage can contribute a completion record.

For compiler-producing actions, the public CLI owns one end of a private pipe.
A process in a separate session holds the workflow, checkout and V8 locks. If the
CLI or its process group is killed, closing the pipe tells that process to stop
the selected publisher. The publisher keeps its signed-toolchain lease while its
worker retires the protected command. Predecessor tests use the same mechanism
while streaming output. The shared Trust daemon runs in its own session.
A new publication step checks that the caller is still present before it starts.

For the certified product build, the controller embeds the source-locked Trust
memory keeper. System Python holds its kernel-authenticated lease outside the
compiler sandbox until the build exits. The compiler can connect to that exact
per-user socket.

`protect` provides the same execution boundary for non-compiler stages. Source
inputs, tool inputs, private dependency configuration and credentials remain
protected; output roots do not grant access to homes or parent directories.
Scratch is short enough for macOS Unix sockets and retained rather than removed
before the outer supervisor establishes that descendants drained. Its retirement
still needs integration with the full release journal. Source-selected Trust
15698 completes a real probe inside this boundary; its optional recursive-Targo
broker currently reports that its separate OS temporary namespace is denied.
This is not recursive-build coverage.

`verify-sandbox` exercises the real native wrapper and kernel policy with private
fixtures. macOS refuses a second `sandbox_apply` inside an existing sandbox.
The source gate therefore compiles this fixed driver inside its protected phase,
then its authenticated parent runs these two controls after that phase drains.
The two corresponding Rust test entry points are explicitly ignored by the
inner test process; the required native phase runs them rather than weakening
either profile or treating an OS refusal as success. Standalone developers can
also run the Rust tests with `--include-ignored` from an unsandboxed driver.

`v8 --repo PATH --intent PATH --cache PATH` resolves the source's locked V8
version and target, downloads through Agh, and verifies the source-pinned checksum
manifest and exactly its archive/binding pair. Cached corruption is repaired only
after a replacement verifies; links are refused and a live lease excludes cache
replacement. Native callers retain the returned lease across compiler group drain.

`build --repo PATH --intent PATH --publisher PATH --state PATH --cargo-home PATH
--v8-cache PATH` creates a locked, detached source checkout in an exclusively
leased campaign directory. Retry reuses that checkout and its canonical build
directory only after verifying their identity and clean source. It builds the
three product executables with fixed arguments, observes the selected compiler
and hashes the outputs. Its immutable local observation is not qualification or
publication authority. Failed or dirty campaigns are preserved for inspection.

A fresh native product build requires the package threshold (35 GiB by default,
configured with `ALAB_MIN_FREE_GB`, with a 20 GiB minimum) on the source and output volumes before fetching or creating a worktree. The controller checks again before
certification. A new ripgrep build uses the same threshold on its output
volume. Compiler scratch, Git metadata, writable Cargo and V8 caches, and the
predecessor test build require 20 GiB at admission. A fresh standalone V8 download and fresh package assembly also require 20 GiB on their
respective destination volumes. A capacity refusal names the path and exits 75.
Verified cache entries and assembled packages remain reusable below that floor.
These entry checks measure available space at that moment.

The product stage fetches official Codex main and the recorded stable tag through Agh.
It verifies the tag against the committed ledger and advances the local upstream tracking
ref only by fast-forward. A publishing worker can start with just the fork’s `origin`.

The product stage runs the exact checkout's `alab/certified-build.sh` with all three
shipping binaries under the held native compiler selection. Each attempt gets a fresh
private report, and the package records its digest and each executable's compiled and
packaged SHA-256. Source, policy and dependency checks belong to the live compiler.
A previous product observation without certification cannot resume this stage.

`ripgrep --repo PATH --intent PATH --observation PATH --publisher PATH --state PATH
--cargo-home PATH` builds the source-pinned registry version with fixed locked
arguments and static PCRE2 under the same source-selected compiler and native
boundary. It verifies actual target compiler metadata and executable bytes,
then runs a contained exact-version/PCRE2 probe. An upstream prebuilt ripgrep
is not an ALab compiler product and is never substituted into this package.
Each build attempt is private and retained; its immutable observation grants
no publication authority.

`assemble --repo PATH --intent PATH --observation PATH --ripgrep-observation PATH
--state PATH --out PATH` verifies the product and ripgrep observations and their
same compiler before copying executable files. Every shipped first-party executable must
have the exact bytes observed after certified compilation. Ripgrep retains its separate
source/compiler observation and strip step. The create-only
package and receipt bind all file hashes and modes. A different ripgrep observation
has its own assembly receipt; rejected earlier packages remain intact. Retry
refuses changed bytes, extra entries, links or permission drift.

`cycle --repo PATH --publisher PATH --state PATH --cargo-home PATH --v8-cache PATH`
is the regular-release entry for the publication worker. The worker's existing
`pub schedule --publish` installation needs a controller with this command and
a native publisher that advertises both Codex predecessor protocol flags. A
stage-only schedule does not dispatch it. On macOS the normal source-selected
`alab/check.sh` builds this controller in its protected target and runs the
resulting `debug/codex-release verify-sandbox`. The publication worker can provision the same controller with
`alab/build-release-controller.sh build SOURCE TARGET-DIR EMPTY-OUTPUT-DIR`.
This runs the native release library tests, selects the Cargo-emitted executable
and exercises its actual sandbox controls. The shipping package remains
independently certified by the release workflow.

The controller reads `publish/registry-repository` from authenticated main
of the registered development repository. The policy may select only that
owner's publication repository. Public source snapshots omit this private
policy and can still build and test the controller.

Each pass reads the live publication registry and the stable Apple silicon
version/build reported by the publisher from the signed index and manifest.
The authenticated main snapshot supplies `VERSION` and its committed ledger
row. The cycle never increments the product version. It qualifies the product
and pinned ripgrep before reserving an unpublished number. A source that fails
those local stages leaves the same version open for a fixed descendant.

If product certification finishes and ripgrep stops, the next cycle for the
same source and publisher checks the original checkout, compiler, certificate
and product binaries before building ripgrep. If both builds finish and a
publication check stops the cycle, a retry validates the recorded outputs of
both builds, including the installed ripgrep executable. It reads the live
publication authority and signed channel again before reserving a number.
A damaged checkpoint stops the cycle. A new source commit qualifies fresh
outputs even when the version stays the same.

After qualification and reconciliation of occupied numbers, the scheduler uses the current
UTC date to allocate a monotonic build number and records it with the source and exact local
observations in the exclusive journal under `STATE/regular-schedule`.
The manual publisher and regular scheduler share a short allocation lock in
the same state directory. Builds run outside that lock. If a manual campaign
takes a number during scheduled qualification, the scheduler checks again
under the lock before recording its reservation. A live manual campaign keeps
its lease. If exactly one completed manual campaign can supply the regular
release, the scheduler adopts its number and observations. Multiple eligible
campaigns require the manual publisher to settle which one ships.

Both entry points honor existing campaign directories and reservations. Keep
the original state and source repository when migrating a manual campaign.
Concurrent writers need the updated controller to coordinate local allocation.
The signed publisher controls promotion across independent release states.

A retry after reservation uses the same frozen source even when main receives
more commits under that version. A newer version on main, a withdrawn registry
delegation or a superseding signed pin stops the affected dispatch. The
publisher rechecks the signed predecessor/candidate pair around promotion and
performs public readback. A successful regular version stays occupied across
source, test and documentation changes. Select a new version in the source
ledger to release further work.

`publish --repo PATH --intent PATH --publisher PATH --state PATH --cargo-home PATH
--v8-cache PATH --build YYYYMMDDNN` chains the canonical product build, source-pinned
ripgrep, assembly and native `atpkg-publish --host agh publish-package` operation.
Its private workflow freezes the build number and exact publisher image, retains
build observations for retries and rechecks them before assembly. The native publisher
owns fresh qualification, signing, staging/readback and exact index publication/retry.
Before promotion, it describes the actual signed channel predecessor and candidate.
The controller compiles the predecessor's existing target reader with its source-selected
Trust and V8 inside the protected worker, then runs that single ignored library test
against the staged executable. An upgrade requires the named test to pass exactly once.
Promotion binds the digest of the exact native input it checked. If the pin moved, a retry
obtains the new pair and checks it afresh. Initial publication and an already-selected
identical package have no transition to test.

The certified product run for `0.12.0` at `21fd0bb35ea4` reached the pinned
Trust 15698 compiler. It refused the `codex-cli` executable because its graph
contained 66 in-process procedural-macro dependencies. The run produced no
certified product observation. Trust 27009 supports isolated procedural macros.
Its project adoption gate stopped at the required verification of the
`anyhow 1.0.103` build script. A new compiler candidate must clear that gate
before a pin change can unblock the release. [Trust 27009 adoption record](../../alab/reviews/trust27009-adoption-build-script-refusal-2026-11-16.md)

`check-predecessor --repo PATH --publisher PATH --state PATH --cargo-home PATH
--v8-cache PATH --source PREDECESSOR_COMMIT --candidate EXECUTABLE
--candidate-version VERSION --candidate-sha256 SHA256` runs the same protected test
independently. A successful JSON observation identifies both endpoints, the compiler
and the exact test executable. This tests the candidate's response to the released
reader. Running-session adoption requires a separate handoff and continued work.

Installation and running-process adoption still need integration; a completed publisher
process is not evidence that the running session has adopted that release.

The product number and update ownership are independent: a release can be
`0.5.0`, while its intent explicitly assigns updates to `atpkg`. Source and
toolchain identity remain separate fields rather than version suffixes.
