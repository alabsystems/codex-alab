<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Merged-tree verification

The requested widest check ran against merged source
`16360783b3186062bbcc056053d71186d9d51650`, with the working tree clean.

## Ownership before checking

The helper checkout at `codex-native-tools-s94ecb6.noindex` remains at
`6b9844f57cfb796edbf6e922076c3ae150ff494f`. Its parent tree already pins
`vendor/astream` to `8497b9710d83d9be4253ae96e7035e84004ea220`, exactly
the submodule's current HEAD. The eleven commits reported relative to that
clone's `origin/main` are inherited history, not work authored in this
session. Both parent and submodule were clean before and after checking.
Leave those commits and refs unchanged; do not publish someone else's work
to clear the harness's ownership warning.

## Canonical gate

Run `alab/check.sh` with the native project controller and source-selected
Trust 15698. The first invocation exited 75 before compilation because its
default target was an existing unmanaged cache. Preserve that cache and
retry with a fresh explicit target:
`codex-rs/target-widest-20261113.noindex`. Native registration accepted it.

The fresh invocation completed full linting: no ALab findings, with 41
upstream diagnostics set aside by the existing attribution policy. It then
exited 2 at the required certified application build. Trust refused the
unaudited in-process procedural-macro dependency closure and emitted no
authenticated coverage or terminal target inventory. Later gate stages
were not reached. This is a failed gate, not a qualified application.

Keep the certification requirement. The compiler capability remains the
delivery prerequisite described in `opaque-bad-request-recovery-2026-11-13.md`;
neither an unverified application nor a favorable saved report substitutes
for a live certified build. No installed application was replaced.

## Available broad verification

Close the earlier tooling-sweep gap by rerunning the complete Python suite
on the merged fixture correction, enabling all seven native package/cache
and controller opt-ins with the retained development tools. The run passed:
955 tests in 1038.624 seconds, one skipped subcase. The skip is the
non-UTF8 filename this filesystem cannot represent. This is a complete
tooling-test result, separate from the failed certified application gate.

The canonical gate's two logs and native proof report are retained under
`.git/alab-qualification/16360783b3186062bbcc056053d71186d9d51650/widest-20261113/`.
The complete tooling log and its manifest are retained under the same
source's `.git/alab-validation` directory. No functional source change was
required by this pass; the cache-admission issue was resolved operationally,
and the unavailable compiler work remains explicitly deferred.

## Cache-admission help

The unmanaged-target refusal's supported recovery was documented in the
README but absent from `check.sh --help`. Add a concrete fresh absolute
`CARGO_TARGET_DIR` invocation to that help output, including the requirement
to preserve existing caches. Keep default selection and explicit override
semantics unchanged: cache selection also participates in Cargo-config
precedence and pristine/runner ownership, so changing one default path
would not be an adequate redesign of that contract.

The help command succeeds even with nonexistent compiler, Python and native
controller overrides, confirming that reading this guidance starts no build
or tool selection. Bash syntax validation passes.

## Nested-lane broker warning

The fresh gate also warned that its contained Targo could not create a
`ttb-*` directory under the macOS per-user temporary directory. Read-only
inspection of Trust source at `86821274c14e5825107ef9563dfc5016e8dd4406`
explains why changing the gate's `TMPDIR` would not resolve this:
`process_authority/macos_broker.rs::user_temp_dir` obtains the directory
from `confstr(_CS_DARWIN_USER_TEMP_DIR)`. Both endpoint creation and
callback-path validation require that location. The gate deliberately
grants writes to its private scratch directory and explicitly named
outputs, rather than the whole per-user temporary directory.

Keep that containment boundary. Defer broker integration until Trust and
the native containment controller support an authenticated endpoint within
the admitted scratch space; do not redirect only endpoint creation or
grant broad temporary-directory access. Trust's
`start_explicit_unverified_targo_broker` explicitly leaves the outer
unverified command authorized after this warning, while recursive Cargo
invocations must select their own lane. The gate completed linting despite
the warning; its later certified-build failure is the separate procedural
macro admission problem above. This inspection changes no executable code
and establishes no additional build qualification.
