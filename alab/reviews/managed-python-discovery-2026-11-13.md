<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Managed Python discovery

During request-recovery verification, `alab/delta.sh --topics` reported no
Python 3.11+ unless `ALAB_PYTHON` was supplied. The machine already exposed
managed Python 3.14 under atpkg's `bin`, while unversioned `python3` resolved
to the older system interpreter. The selector searched legacy Homebrew/uv
locations and the unversioned PATH command, but not the managed directory.

Search the managed package directory for unversioned and versioned Python
before legacy locations, retaining the actual version probe and excluding
`-config` helpers. Discover versioned names without a fixed maximum version.
An explicit `ALAB_PYTHON` remains authoritative: validate it, and refuse an
invalid override without silently choosing another interpreter.

V8 prerequisite preparation now uses this same selector instead of carrying
a second copy of the discovery algorithm. This also makes its failure mode
consistent with review and packaging helpers. No compiler, proof policy,
package installation or user environment is changed.

The selector fixtures exercise managed-only discovery under a path with
spaces, an unsuitable unversioned candidate, a future versioned name with a
`-config` decoy, explicit precedence, and refusal of an invalid override.
The gate fixture exercises native V8 input preparation without an override.
On this machine, both interpreter discovery and `delta.sh --topics` work
with `ALAB_PYTHON` removed from the environment.

Validation passed: the 64-test gate regression run (discovery, V8, proxy
home, lint orchestration and package entry), followed by all 11 final
discovery/V8 tests after completing version-independent discovery. Bash
syntax checks passed for both shell files. The selected interpreter's
measured runtime root is the managed cpython store, so discovery does not
require a broader containment grant.
The fixture suites explicitly require Python 3.11+: all 11 pass under the
managed interpreter and report that prerequisite when invoked by the older
system Python, rather than testing with an unsuitable fixture executable.

## Relative interpreter paths

A follow-up reproduction selected `ALAB_PYTHON=./python` successfully, then
failed with exit 127 after changing directories. The selector had returned
the relative path unchanged. Anchor relative results to the invocation's
physical working directory before handing them to build helpers. Preserve
the executable symlink: resolving it to the base interpreter would change
a virtual environment's identity.

All 14 focused selector/V8 tests pass, including a relative override executed
from another directory, a real temporary virtual environment whose prefix
survives selection, and the public entrypoint's preselection followed by
candidate/pristine V8 preparation. Bash syntax checks pass. Absolute
overrides retain their existing output and priority.

## Probe completion

The original version check accepted an executable solely because it exited
zero: `ALAB_PYTHON=/usr/bin/true` was reported as a suitable interpreter.
Require successful completion and the exact response emitted by the Python
probe, including an import of the TOML module the callers need. A zero exit
without that response, or a response followed by a failed exit, is refused.
Automatic discovery can continue to the next candidate; an explicit invalid
override still refuses rather than silently falling back.

All 19 selector, V8 and package-entry tests pass, including both new negative
controls. The real managed interpreter still runs `delta.sh --topics`
without an override. This is an interpreter usability check; native tool
admission continues to own executable identity and build authority.

## Complete tooling sweep and fixture repair

The full tooling run at `b1a23d94c6b10aedee48cf218b6712d29a8ce9d4`
ran 955 tests and reported one failure plus eight skips. The failure was
`test_plain_cache_guard_and_read_only_package_modes_keep_old_defaults`:
its fake Python recorder delegated only the previous version-probe source
string to the real interpreter. It recorded the new probe instead of
executing it, so the strengthened interpreter check correctly refused it.

Delegate inline `-I -c` programs by that interface, while retaining the
fixture's guardian-call interception. This removes its dependency on the
implementation text of the probe. All 16 disk-guard wiring tests pass after
the fixture correction. The original full-run failure remains recorded;
the focused repair does not turn it into a claimed clean full rerun.

The seven native opt-ins that the ordinary run skipped were then exercised
with the retained development tools: four package-cache cases, two native
controller/cancellation cases, and the external-path source-cache case all
passed. The remaining skipped subcase needs a non-UTF8 filename this
filesystem cannot represent. These are runtime/tooling observations and
do not qualify an application package.
