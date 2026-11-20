<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Preserve the requested packaged Sol 6.1 Ultra default

Decision: restore only the packaged `gpt-6.1-sol` selection, keeping Ultra and
the catalog family resolver and client ownership changes intact. The source
base is `74f6cb881fa141312d57906d74a6c44c5ef72c7f`.

The November 16 request in this conversation was:

> first, the default for codex-alab needs to be Sol 6.1 ultra, not "low"

The follow-up explicitly asked for both local settings and codex-alab. Commit
`87023d47a720d440c07654e9bc6317f0924818bf` supplied that packaged selection;
`alab/reviews/reset-decision-notices-2026-11-16.md` records the same scope.

On November 17, `f58eb98e6311a58090ceb29594ed086e436649ce` changed the packaged
model to Sol 6.2, and `5f23d38cfa32640867bfef4aa8b5527b7caeb997` removed the pin
in favor of the newest eligible Sol. Those commits express an implementation
decision. This conversation contains no later request revoking the specific
6.1 default, and the reviewed commit messages and model-default records do not
establish later human steering. A newer model in the tool catalog does not
itself change the user's requested default.

The explicit request therefore governs this reconciliation. A packaged model
value restores that default without reverting the family implementation. The
existing `Config::alab_resolves_sol_default` requires `self.model.is_none()`;
an effective model pin follows the existing manager path instead.
Catalog-based selection remains available when the effective model is unset.
Explicit user, profile, project and CLI configuration retains its normal
precedence, and the provider/catalog ownership code remains unchanged.

Only the packaged model line, its comment, the product-policy invariant and
this decision record change. The Ultra value and opt-in Kimi High overrides
remain as they were. No Rust implementation, upstream-owned path, user
configuration, model catalog or environment override changes.

Validation used managed Python 3.14 to parse both TOML files and compare them
with the source base. The only defaults value added is `model`; the existing
Ultra value and every other defaults value are unchanged. The feature catalog
differs only in the product-policy model invariant. Static inspection confirmed
the existing unset-model guard and explicit-model manager path; the Rust
implementation is unchanged. `git diff --check` passed. No Rust compilation,
Rust tests or native qualification ran; these are configuration and static
source checks only.
