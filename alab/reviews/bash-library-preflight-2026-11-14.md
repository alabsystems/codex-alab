<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Bash library preflight source review

Unsupported zsh sourcing could skip the Bash-only `compgen` Git scrub and still
reach native dispatch. The library already requires Bash 3.2. Reject an
unsupported source interpreter before initialization; do not reexecute or add a
compiler/Git fallback.

Reviewed `a17fb6fac97b10c7f204b82502540c37294ae29d` against
`9713e3a99400c2f7e018f16d61b31dc918c59388`, including the preceding
`aeda216370a75515cc52c7b462a8549955946127` attempt. No blocking source issue in
the final correction. At that source, all 13 focused Python methods pass in
2.710 seconds with zero skips. Actual Bash 3.2.57 syntax validation passes.
The live qualified runner selects Trust 15698 and passes all four state-contract
tests with the guarded library. Independent source review and these execution
results are separate evidence.

Only `alab/lib.sh` and its existing project-toolchain fixture change. The
nine-line guard precedes BASH_SOURCE, root initialization and helper definitions.
Its POSIX grammar/Bash 3.2 builtins preserve nounset and caller options. Compiler
selection, containment, cache, scrubbing and native authority are unchanged.

A version variable alone is insufficient: lookup must identify `builtin`, then
`builtin shopt -p` must succeed. PATH executables named builtin/shopt are not
executed. Whole-subshell redirection covers zsh builtin resolution diagnostics.

Sourcing returns 2 before initialization; a non-errexit caller can continue.
Direct execution exits 2; errexit remains caller policy. This shell preflight
does not replace native qualification.

The test-only RED commit `2c4ec09a23fdb4e61a9961e415f31eaa8c5ffff4`
ran 13 methods: the wrong-shell methods produced eight subcase failures while the
other 11 methods, including the Bash positive, passed. Sourced zsh reached the
fixture recorder and dash failed with unsupported-shell expansion/syntax errors.
The initial aeda guard rejected unsupported execution before those side effects,
but its two spoofed-version zsh cases retained a resolver diagnostic. The final
outer-redirection correction passes the strict stderr controls.

Final controls cover both shells, source/direct and absent/spoofed version;
exact diagnostic; recorder/cache/external-probe absence; captured source status;
unchanged root; undefined helper; caller liveness. Bash initialization and its
harmless real helper remain green. New subprocess calls have ten-second
deadlines; unavailable optional shells skip.

The inert recorder does not prove actual native authentication or application
qualification. The separate real native smoke exercises the guarded controller
through explicitly unverified runtime tests; it emits no application proof.
`git diff --check` and committed ownership inventory pass (86 hook topics,
13 features). Exact-source logs, failed attempts, diffs and hashes remain in the
private common Git validation archive. No Rust source changed; preceding Tippy
and full Python results are antecedent, not new rerun claims. Application delivery
remains deferred at the [Targo macro-admission boundary](targo-macro-admission-2026-11-14.md).
