<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Bind promotion ancestry checks to the requested repository

## Finding and decision

Promotion passed an explicit repository to Git with `-C`, but inherited
`GIT_DIR` could still select another repository. A private fixture made the
requested repository's `origin/main` exclude the candidate while another
repository's ref included it. Without the redirect, promotion refused before
calling the selector. With the redirect, it reported success and reached the
mocked conditional selector. No actual link or installed package was changed.

Add a reusable Git environment helper and use it for promotion's ancestry
queries. Remove inherited Git variables, then disable global/system config,
replacement objects, lazy object fetching and optional locks explicitly.
Repository hooks and explicit Git mutations are not disabled. The requested
repository now supplies both the ancestry graph and `origin/main` used to
decide whether a package may advance selection.

## Validation and limits

The new real-Git regression failed against the original helper. It now passes
both directions: a foreign repository cannot authorize an ineligible candidate,
and a foreign rejection cannot block an eligible candidate in the requested
repository. The selector is mocked throughout, and the fixture's selection
marker remains unchanged.

All 21 promotion tests passed in 5.382 seconds with no skips. Independent
read-only review found no correctness issue, and `git diff --check` passed.
The original reproduction is retained under
`$HOME/.aterm/.ubd/promotion-git-boundary-37fxnpos.noindex`; regression logs are
under `$HOME/.aterm/.ubd/promotion-boundary-repair-20261112.VE23EX`.

This qualifies the local repository-context decision only. No native build,
real package selection, release gate or installed-adoption check was run.
