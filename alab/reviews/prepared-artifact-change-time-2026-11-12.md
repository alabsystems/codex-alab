# Detect changes to a prepared library-test artifact

The library-result preparation and native runner previously compared device,
inode, length and modification time. A same-length in-place edit followed by
restoring the original atime and mtime preserves all four values. The prepared
artifact can therefore execute different bytes while satisfying that check.

A retained private reproduction used the real `test_unit_results.py` fixture
helpers and existing native runner. After real preparation, it changed only
`fixture executed` to the equally long `changed executed` output marker and
restored the original timestamps. The runner executed the changed fixture,
advanced its counter from one to two and exited zero. The real collector also
accepted the run. The unchanged control passed before the edit.

The prepared and changed tuple was
`[16777232, 1409939601, 2149, 1794524972768363527]`. The content hashes changed
from `7b7c544e3f752895bdf9e9a44b355001c7c1fa80f347d628f61919b55a458b34`
to `a44f13687c9bea0d471b7d150d532698fd1cd403ed74c3243d1362ec612a5ad2`;
ctime_ns changed from `1794524972768427195` to `1794524972946352206`.
The old runner hash was
`eb83cda1b09cd68e5c10fae21a27ee094565560c0d4e33a61d6e23e0ea982287`.
Raw state, receipts, logs and observations remain in
`$ATERM_STATE/.ubd/runner-identity-20261112.noindex/real-runner-artifact-rewrite-daba0f3j.noindex`.

## Decision and scope

Add ctime_ns as the fifth value in both Python preparation and the native
runner's comparison. The fixed five-element native representation must refuse
old four-element states before execution; there is no compatibility fallback.
The producer and consumer must be deployed together, and a new preparation is
required for old state. Retain the current regular-file check, source/package
binding, runner arguments, receipt format and POSIX execution path.

This closes the demonstrated restored-mtime gap. It is a metadata change
detector, not a content digest or immutable-artifact guarantee. Hashing a path
and later executing it would still permit path replacement. Executing an
opened object could address that substitution but would not by itself prevent
another writer modifying its inode after validation. A stronger contract would
require separate decisions about digests, stable object execution and an
enforceable writer boundary; those changes are outside this topic.

## Compatibility and observation limits

Use Unix change time, not creation time, with checked seconds/nanoseconds
conversion matching the existing mtime representation. The native runtime is
already Unix-only; its non-Unix entry refuses qualified execution. Python's
integer nanosecond fields do not imply that every filesystem records changes
at nanosecond resolution. A rewrite within one filesystem timestamp tick is
not guaranteed to be distinguished, and ordinary timestamp restoration is not
a claim about privileged metadata manipulation.

Read-only inspection and ordinary execution do not require a new ctime.
Rebuilding, replacing, changing permissions or changing metadata after
preparation can invalidate a snapshot even if the resulting bytes match.
That conservative refusal is intentional: Cargo must run the artifact that was
prepared. If Cargo legitimately rebuilds it, prepare the new artifact rather
than relaxing the comparison. Keep preparation and invocation on the same
selected package/feature graph so the runner configuration itself does not
introduce an avoidable rebuild.

## Validation plan and results

Extend the existing real-runner replacement regression with a same-length
rewrite and restored atime/mtime. Verify the actual prepared five-value state,
the equal original four values, the observed ctime change and refusal before
the fixture counter advances. Keep an unchanged-artifact execution control.
Add a four-value-state regression that refuses before execution. Preserve the
existing package-directory, malformed-state, receipt and comparison controls.
Run the focused unit-result suite against the newly built native runner under
the existing protected lane; broader gate fixtures must use that same runner.

- Trust 15698 built the new runner from committed production source
  `7cf4721895f9d0cd840c279329d55271da2e6f81`; isolated crate Tippy passed.
- All 19 unit-result tests passed, including same-length rewrite rejection,
  old-state rejection and unchanged execution. The retained runner copy also
  passed the rewrite/control case independently of its build directory.
- All three real-Cargo native context tests passed. Nineteen observations of the
  actual compiled probe, across no-run, preparation and repeated invocation,
  retained one identical five-field snapshot. The original runner's bytes
  also remained unchanged after lint and execution.
- All 48 gate fixtures passed with the rebuilt runner, including candidate,
  isolated and pristine execution, helper selection and missing-receipt guards.
  Together with the 19 unit-result and three native context tests, 70 tests passed.

The first native validation driver correctly failed when its build output
escaped the fixture's declared target. A later driver omitted the workspace
working directory. Both attempts remain recorded; the passing run used
separate declared output and build directories for lint and the native probe,
without relaxing source or artifact checks.

These are scoped regression and native execution results, not a full-source
qualification or a formal proof. Records and the retained test-only runner are
under `$ATERM_STATE/.ubd/artifact-ctime-20261112.noindex`.
Source V1 activation remains held.
