<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Honor explicit network remote syntax before local directory lookup

`github.location` checked for an existing checkout-relative directory before
validating HTTPS, SSH or SCP-style remote syntax. A directory named like
`https://github.com/fixture/codex.git` could therefore select local transport
for an explicitly configured GitHub origin. The publication counterfactual
actually advances that owned local bare shadow, leaves the intended fixture
remote at its old commit and never calls the injected native client.

Decision: classify a colon before the first forward slash as remote syntax
before testing directory existence. Native `Path.drive` preserves Windows drive
paths lexically; Windows execution is not measured here. Explicit absolute,
`./`, `../` and file-URI local paths retain their meaning. Existing URL host,
authentication, port, query, scheme and owner-alias validation remains in force.
The existing bare `OWNER/REPO` versus local-directory ambiguity is intentionally
unchanged; this fix concerns explicit URL/SCP syntax. No credentials, remote
configuration, hooks, network policy or raw-Git fallback are changed.

Five new maintained methods run unchanged against exact coherent `github.py`
and `github_push.py` Git snapshots from
`64e4e8271ef831b49fc95bdb9c21543478cfb260`. Baseline: three failing parent
methods, two passing parents, 15 assertion-failure events, zero errors/skips,
actual exit 1. The 15 events are five valid URL shadows, nine invalid URL/SCP
shadows and the publication-routing assertion; they are not 15 methods.
Fixed: all five methods and 17 URL/local subtests pass, actual exit 0. Named
remotes, owner aliases, explicit punctuation-bearing local paths and the bare
local path contract are included. Publication preserves config and HEAD,
keeps the shadow at its base, advances the intended fixture remote, and uses
the injected native client's fetch and push operations. All Git repositories
are owned temporary fixtures; no live GitHub, Agh or Rust invocation occurs.

The final ordinary unittest union runs the complete transport (21), publication
(10) and workflow (33) modules: 64 distinct outer methods pass in 18.853 seconds,
actual exit 0, with zero failures, errors, skips, expected failures or unexpected
successes. Selected source/test/script files and the selected Git/Python
entrypoints retain their before/after fingerprints. The workflow topic shares
this union; its subsets do not add another 33 or 64 methods. Independent review
approves the frozen routing implementation and public-operation controls.

Fixed `github.py` SHA-256:
`4c57159ed665357a17ec9a425cfc85aadd6e3ec3a2f2724c75dac6ad48113766`.
Transport controls: `5a777967ed362e37503b6867858f219b21ab87ddfcb1b434c5f3615ba5b2d689`.
Publication controls: `b4c05a9aadf272d195d80524bcbd7194e8f4d3ef91d930b6beddb67709ad7bd1`.
Final union log: `d79abb3606ae8251a53b435bdd27f24d5379ef90ef45070aa6ffa1bf181ad286`.

The common Git directory retains 35 payloads (415,618 bytes) at
`alab-validation/64e4e8271ef831b49fc95bdb9c21543478cfb260/transport-workflow-kv2iobfy/`,
index SHA-256
`485b4947a92ebd0cf648c1ed716d8ab55549be484e33ecd363d905a2c7b49344`.
They include coherent baseline/fixed source and test copies, raw counterfactual
and union logs, frozen drivers and source/tool observations. Root verifies exact
Git baseline bytes, frozen fixed/test identities, outcome counts, every payload
size/hash and the exact regular-file set. A preparation-only archive candidate
omitted nested snapshots; the complete packet replaces that candidate without
changing original evidence or repeating test executions.

Fresh main `80d42a7e513f1bc1ee108735a945b5e40fc9f789` contains disjoint sync
changes. Preserve them during publication while retaining these results' source
identities. No complete Python workflow, native application gate, new native
build, compiler/profile change or application qualification is claimed.
