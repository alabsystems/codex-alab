<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Validate optional policy-file declarations

## Finding and decision

Review of committed feature inventory found a separate schema omission:
`policy_files = "missing-policy.toml"` was silently skipped, so an active feature
with that malformed declaration passed ownership validation. The existing
path loop only inspected lists and had no structural check for this optional
field.

Keep `policy_files` optional; omission and an empty list remain valid. When it
is present, require a list of nonempty strings. Apply this structural rule to
all lifecycles, as for the inventory's other list fields. Keep path existence
checks limited to active and retiring features, so retired records can name
files that were removed. No catalog migration is needed: the two current
declarations are valid lists, and the other features omit the field.

This follows the source-coherence correction in
[committed-feature-inventory-2026-11-12.md](committed-feature-inventory-2026-11-12.md).
It changes schema validation only; it adds no admission or qualification claim.

## Validation

Before the change, a direct check of an active feature with a scalar
`policy_files` declaration returned no errors. Regression checks cover malformed
containers and list members across all three lifecycles, optional and valid
lists, retained historical paths without filesystem lookups, and an actual
committed CLI catalog with the malformed scalar declaration.

The two negative-control tests failed 25 assertions against the prior
validator: eight malformed values across three lifecycles, plus the committed
scalar CLI case. There were no setup errors. All 18 inventory tests now pass,
including the earlier source-selection regressions. Independent review found
no remaining issue with the optional, empty-list or retired-path semantics.
