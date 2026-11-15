<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Configured Python workflow sweep

## Decision and reason

Complete the documented full workflow sweep with its actual native adapter.
The earlier cold adapter request was deferred when memory admission could not
start it. A fresh observation of 22.6 GiB available justified one new attempt;
ordinary native admission then allowed the source-selected component build.
Do not substitute a fake adapter, reduce a memory claim or weaken fixture
checks to obtain a passing count.

Build `codex-alab-unit-runner` from clean source
`d67040a0294a76ae5a760cf52b6b1abce7d0fdf0` through the qualified project frontend
and physical Trust 15698 in the documented explicit unverified support-tool
lane. The build completed with exit 0. Use its actual Cargo-reported executable
and measured SHA-256 for the sweep. Keep the documented physical Targo input,
private Cargo home, separate persistent native-context target, fixed 60-second
native subprocess timeout and existing native opt-in settings. No ad hoc compiler or runner wrapper, authority bypass,
reduced memory claim or timeout change was introduced.

## Result and scope

The one full `unittest discover` invocation on clean source
`d67040a0294a76ae5a760cf52b6b1abce7d0fdf0` completed with exit 0: 1,003 tests in
1,233.423 seconds, eight skipped, no failures or errors. All three real
native-context tests passed, including entry-context custody, the configured
runner path and the Python signal-change negative control. Native setup's
no-run compilation completed in 7.85 seconds; nine Cargo invocation evidence
sets and entry/receipt observations are retained. Seven skips are native opt-in
cases; one is a filesystem non-UTF8 subtest. These remain skips, not passes.
Neither native timeout nor admission rules changed.

The common Git archive is
`.git/alab-validation/d67040a0294a76ae5a760cf52b6b1abce7d0fdf0/full-python-workflow/`.
It retains the invocation manifest, exact log, actual exit status, adapter build
and identity, selected-file/LINEAGE checks, native artifact records and hashes.
The binaries remain in their persistent targets outside the archive. Native
receipts contain inherited execution context and remain in private evidence;
this review reproduces no environment values. Selected paths and artifact
identities are evidence inputs, not an observation of a compiler process.

The later nested-WebSocket-ID change at
`b0042b2571f9a703eced6552020997a53d9be16b` changes no Python workflow script or test,
native adapter, workspace dependency manifest/lock or compiler pin. Its feature
scenario addition separately passes ownership validation at fixed source
`658c0399689ff5f405160f842feac8f6571686dc` (86 hook topics, 13 features). That
source also passes 284 API tests and scoped all-target Tippy with zero ALab
diagnostics; see the [correlation review](websocket-nested-error-ids-2026-11-14.md).
Do not relabel this sweep as a full-suite run against the later source.

This is workflow and runtime-test evidence. It does not qualify an application
or replace the live certified build required for shipping. Application delivery
remains deferred at the recorded source-selected
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md). No package,
managed executable or running agent was replaced.
