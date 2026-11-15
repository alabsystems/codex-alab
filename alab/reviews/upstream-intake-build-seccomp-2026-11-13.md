# Build helpers and Linux key-request restriction

Extend the reviewed upstream prefix from 43 to 46 commits through
`7eaaee4270006aec62499da46c66b5f140d47160`, preserving earlier decisions.
Fifty later commits remain in the observed upstream snapshot. This review
does not merge upstream or qualify the application.

## Build-tool resolution (`c123aa2c10`): adopt retained Python change

The Bazel module and bubblewrap rule changes are covered by existing whole-path
removal policy. Preserve that removal. The two `third_party/voice` Python files
remain in this repository: on Unix, the recipe now adds the active Python
interpreter's directory after its private tools bin and before compiler-tool
directories. Windows environment construction is unchanged.

Ran the new `test_generator_uses_the_recipe_interpreter` against the exact
fetched commit's voice sources using managed Python 3.14. It passes. Then ran
the same test with only `build_native.py` replaced by its parent version: it
fails with an interpreter-path assertion. This verifies that an `env python3`
generator selects the recipe's interpreter for this fixture. Temporary source
copies were removed; logs and source identity are retained in
`.git/alab-validation/upstream-c123aa2c10-voice`.

This is a focused generator regression result, not a native voice package
build or a passing application gate. Preserve explicit compiler/input and
deployment-target checks and require the wider native recipe tests on the
eventual integrated candidate.

## V8 pip index (`7b2ed14efe`): retain removed Bazel policy

The only changed path is `MODULE.bazel`, already removed. It allows an explicit
pip index while keeping hash requirements, but no ALab build input reads this
removed path. Keep the native source-selected build lane; do not restore Bazel
or change package credentials as part of this intake.

## `request_key` (`7eaaee4270`): adopt restriction

Restricted and proxy-routed seccomp policies add an unconditional EPERM rule
for `request_key`. VM-socket-only policy retains its prior behavior. The rule
passes through the existing syscall helper, preserving its x86-64/x32 mapping
logic and all other network restrictions.

The kernel documents that key requests can invoke a userspace helper, which
may obtain data over the network. This supports restricting that syscall at
the network-sandbox boundary:
[Key Request Service](https://www.kernel.org/doc/html/latest/security/keys/request-key.html).
The regression first installs an ENOSYS rule and then the target filter on a
separate test thread. This avoids invoking a helper and distinguishes the
new EPERM rule from an inherited EPERM rule. The kernel documents that the
most recently installed filter supplies the data when actions have equal
precedence:
[Seccomp filter return values](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html).

Read both changed files and surrounding filter construction. Require native
Linux tests for all three modes, the existing x32 coverage, and ALab network
policy tests. This macOS host does not execute Linux seccomp; no syscall test
pass is claimed here.

## Validation

Validate exact 46-commit coverage, unchanged prior 43 entries, unique active
decisions and rejection of the next unreviewed commit. The focused Python
red/green result and metadata admission fixtures remain distinct from native
Linux and certified application qualification.
