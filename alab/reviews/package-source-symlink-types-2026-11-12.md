<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve symlink file types in package-source checks

## Finding and decision

At `2268efee92`, a private fixture replaced a committed `alias -> changing`
symlink with a regular file containing the same bytes, `changing`. With
`core.symlinks=false`, both ordinary status and the fresh-index comparison
reported no change. `validate_binding` accepted the replacement, and
`remove_fresh` deleted the source, its Git registration and that regular file.
This was a lost file-type change; it was not a claim that edited file contents
were deleted. The `core.symlinks=true` control detected the type change and
refused both validation and removal.

Pin `core.symlinks=true` in the physical source comparison and Git's final
worktree-removal safeguard, alongside the existing executable-mode check.
Caller configuration may control ordinary Git checkout preferences, but it
cannot redefine a committed symlink as an owned regular source file. A new
checkout made into regular-file stubs is refused and retained. This changes
no repository configuration and keeps ordinary clean symlinks usable.

The final removal override is necessary independently of validation: a type
replacement after the last validation must still be refused by Git's own
removal check. Command-local settings take precedence over an inherited
`GIT_CONFIG_COUNT` setting for `core.symlinks=false` too. Existing receipts
continue to be revalidated live, so no custody-schema change is needed.

## Validation and limits

The four-method negative control produced five expected failures in
6.717 seconds: regular-file replacements were admitted, late replacements
were deleted, and fresh regular-file stubs were accepted. Actual symlink
controls passed. The tests cover clone-local configuration and inherited
command configuration, inspect real source and registration preservation,
and check clean removal separately.
All four focused methods passed after the fix in 9.668 seconds. Independent
review found no issue in the two command-local overrides, and
`git diff --check` passed.

The original audit observations remain under
`$HOME/.aterm/.ubd/package-symlink-audit-wqszdiil.noindex`.
Focused logs, commands and source hashes are retained under
`$HOME/.aterm/.ubd/package-symlink-validation-20261112-6tjzqq26.noindex`.
All fixtures use local Git, plain files and inert Python execution; no native
build, live cache or real package selection was used.

Changing a symlink's target text was already detected, as was editing a
tracked referent within the source. A committed link to an external file can
still read changed external contents while Git reports its unchanged link
text as clean. The audit demonstrated that declared external-input boundary;
this change adds no new symlink-target rejection policy or dependency
attestation. A quick sparse-pattern control also refused changed source;
no unrelated Git configuration fix is included.

The combined source, controller and portability results are recorded in the
[gitlink composition review](package-source-gitlink-custody-2026-11-12.md#composition-and-controller-verification).
