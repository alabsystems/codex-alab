# Native codex-alab release controller

`codex-release prepare --repo PATH --source FULL_SHA --intent PATH` freezes
the source-owned regular product version, upstream client version and baseline,
target, compiler lock and dependency lock. GitHub access goes through Agh.
The intent binds the canonical GitHub repository derived from its admitted origin;
changing that origin invalidates it. Publication ownership remains atpkg's configured
authority. The intent is written durably without replacing an earlier campaign's inputs.
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
same compiler before copying and stripping executable copies. The create-only
package and receipt bind all file hashes and modes. A different ripgrep observation
has its own assembly receipt; rejected earlier packages remain intact. Retry
refuses changed bytes, extra entries, links or permission drift.

`publish --repo PATH --intent PATH --publisher PATH --state PATH --cargo-home PATH
--v8-cache PATH --build YYYYMMDDNN` chains the canonical product build, source-pinned
ripgrep, assembly and native `atpkg-publish --host agh publish-package` operation.
Its private workflow freezes the build number and exact publisher image, retains
build observations for retries and rechecks them before assembly. The native publisher
owns fresh qualification, signing, staging/readback and exact index publication/retry.
Installation and running-process adoption still need integration; a completed publisher
process is not evidence that the running session has adopted that release.

The product number and update ownership are independent: a release can be
`0.5.0`, while its intent explicitly assigns updates to `atpkg`. Source and
toolchain identity remain separate fields rather than version suffixes.
