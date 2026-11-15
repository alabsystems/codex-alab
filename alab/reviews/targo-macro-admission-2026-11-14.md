<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Identify the certified-build admission boundary

The retained gate for source
`24b01cc6a552f3066e3b87ccf9b34ff7237a9abb` is under the common Git
directory's `alab-qualification/<source>/run-o8fbtz74/`. Its `record.json`
reports exit 2, unchanged clean source before and after, and
`qualified_source: null`.

Native resource admission succeeded (`gate.log`, line 157). The selected
Trust 15698 Targo then adopted `certify` with dependency verification and
refused the `codex-cli::codex` dependency graph (line 164):

> Evidence-grade verification currently enforces a no-proc-macro TCB boundary;
> expand/remove these macros before verification

The refusal names Codex's in-process procedural-macro dependencies, including
Serde and Clap derives. It is Targo graph admission before solver execution
or proof collection, not a failed proof obligation in `trustc`. The absent
`TRUST_JSON`, coverage summary and authenticated target inventory are
consequences of that refusal; their generic report suggestions do not establish
another cause. The earlier nested-lane broker and launcher-provenance warnings
are separate from this primary error.

## Selected tools and the separately rebuilt front

`alab/project-targo.sh` invokes `ATPKG_PROJECT_FRONT __build_admit` with
`ATPKG_PROJECT_TARGO`. The native project controller supplies the former
from its own executable and the latter from the selected Trust payload's
execution mapping. Rebuilding the `atpkg-publish` admission front therefore
does not itself change the selected Targo's macro-admission capability.

An unchanged `trust.lock` alone cannot establish that every executable in this
chain is unchanged. Deferral rests on the identified selected-Targo requirement
and the unchanged `certify` plus dependency-verification policy, rather than
on the pin text alone. The missing capability is evidence-grade admission for
Codex's actual procedural-macro dependency graph. No supported configuration
fix for that graph was found in this audit.

## Static supporting evidence

The supporting audit read a committed blob with `git show` in the local
`~/trust` checkout at `7abe97d22dc14f44cff3152bbc77637cc58146af`, path
`src/tools/targo/crates/targo-core/src/compiler/mod.rs`. Its
`admit_in_process_proc_macro_tcb` still rejects unaudited macros when the
lane claims proof authority; the verified-unit dependency scan feeds that
decision. This is supporting source evidence, not a rerun of the selected
artifact or the rebuilt admission front.

No build, compiler probe, gate rerun, override or installation occurred. The
historical failed gate remains historical evidence; this audit grants no
qualification and does not establish that removing this boundary alone would
make application certification pass.

## Exact selected-source follow-up

A later bounded local lookup confirms that the selected compiler commit
`20565c3255adf151f9eb920e89cdf83c38f84abe` is readable at its historical path
`src/tools/targo/src/cargo/core/compiler/mod.rs`, blob
`799c81cc6fff40d3a503d95382b00ca18e434d31`. The private refresh record retains
the initial availability observation and its timestamped correction. The new
read removes inherited Git context and disables lazy fetching and replacement
objects; no network operation or executable probe is involved.

In that exact source, lines 1115–1173 reject unaudited in-process macros in
proof-authority scope; lines 2408–2417 include `certify` in that policy. The
verified dependency scope and scan at lines 1605–1639 and 1666–1694 feed the
same decision. The audited-source list names `trust-spec` and `ny-contracts`,
which does not admit Codex's Serde/Clap and other macro graph.

This strengthens source support for the existing deferral. It remains separate
from the historical selected-artifact gate, authenticates no whole payload,
and grants no application qualification. No compiler selection, configuration,
proof requirement or installation changed.
