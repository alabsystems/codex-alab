<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Canonical Git ancestry, owned scratch cleanup and literal receiver paths

Three adjacent public-API controls reproduced defects at exact source
`5d49d5658335155cd0fafb6af20bbd57f17302cd` using owned local repositories.
The decisions below preserve expected-ref CAS, receiver fast-forward enforcement,
GitHub routing through Agh and the separate release-ledger history guards.

A caller's `info/grafts` could make a parentless remote commit appear to descend
from tracked main. The same nonforced fetch refused without the graft and
advanced tracking main with it. Decision: pin `GIT_GRAFT_FILE` to the empty device
beside `GIT_NO_REPLACE_OBJECTS` in the adapter's sanitized Git environment.
Direct adapter Git calls then read canonical parent relationships while leaving
graft files, configuration and the caller's HEAD intact. A misleading graft also
stops hiding a real fast-forward. This does not claim to validate shallow
boundaries or external producers. `versions.py` retains its own explicit
whole-history graft/shallow refusal and replacement-object policy.

Ordinary `TemporaryDirectory.__exit__` cleanup errors replaced pending
KeyboardInterrupt, SystemExit and ordinary failures. Decision: share one owned
scratch context across `Agh.fetch_batch`, publication and `versions.released`.
Attempt cleanup once; retain an already pending cancellation, or promote a new
cleanup cancellation over an ordinary primary error. Otherwise retain the
original failure and add the cleanup error as a best-effort note. Protected note
formatting/reporting cannot replace pending cancellation. A new reporting
cancellation takes precedence over an ordinary primary error. After a successful
body, cleanup failures still propagate. Creation failure starts no body and
owns no directory. Public APIs retain exact SystemExit objects, including zero
and None; this change adds no CLI exit-code conversion or retirement-success
claim after failed cleanup.

A real receiver path containing newline followed by a status-like tabbed line
caused publication to report refusal after Git had actually advanced main.
Decision: consume the exact known binary `To <destination>\n` header as path
data, then require one exact-source/main status record, `Done` and its final
newline. Embedded tabs, LF, CR and non-ASCII path bytes remain data. Unknown
framing or flags still refuse. No line-scanning or locale heuristic is used.
Actual Git 2.56.0 output verifies all five path forms. Its primary
[transport source](https://github.com/git/git/blob/v2.56.0/transport.c) confirms
literal header/footer emission and unchanged local URL text. Agh GET retains
blob `25e2c14a7bde24f5da41fdc1f8f56e849205ccb3`, SHA-256
`760ca6713a06d54f8362dc14b606e3960cd87ac672d43886ae2b14182eafb412`.
These platform observations cover the tested Mac/Git environment.

The same final focused controls run against coherent exact baseline files and
fixed files, with source/test/helper/tool identities unchanged before/after:

- Six graft methods: baseline actual exit 1, five assertion failures across
  five methods and one pass; pin-only fix actual exit 0, six passes. Controls
  cover local/native-fixture fetches, linked worktrees, real hidden-parent
  fast-forwards and replacement refs. Config/graft bytes and logical non-cache
  refs, reflogs and HEAD are guarded; scratch cache transfer remains permitted.
- Twelve cleanup methods: baseline actual exit 1, 27 assertion events across
  eight failed methods, zero errors; fixed actual exit 0, twelve passes. Real
  owned directories are removed before module-local cleanup faults are injected.
  Exact primary identity and natural secondary notes cover both cancellation
  orderings through all three public consumers. No global tempfile mutation,
  fake descriptor, process signal or direct private-helper test is used.
- Six porcelain methods: baseline actual exit 1, three failures plus three errors
  across four failed methods; fixed actual exit 0, six passes. Real unmodified
  Git bytes cover success, equal-candidate no-op and a stale-candidate race.
  Four deliberately corrupted responses after real owned writes test refusal;
  those faults are not described as rolled-back writes or confirmed success.

An earlier eleven-method cleanup pair is retained separately. Review requested
the additional primary-cancellation/later-cleanup-cancellation matrix and natural
note assertions before the final twelve-method pair. Neither earlier checks nor
focused pairs add to the final regression count.

The final nine-module run uses clean committed source
`f6db0d47c044478695a0c66286e52f61788b94ea`, tree
`052660c62233f8166c917b47138701eaefaa8c41`: 126 distinct methods, 124 passed,
two existing certification-dependent pre-promote tests skipped, zero failures,
errors, expected failures or unexpected successes. Actual exit is 0; unittest
reports 262.517 seconds. The two skipped tests remain explicitly deferred until
the required compiler can certify the application; their decorators and the
committed pre-promote note are unchanged. All 72 Git methods, 45 ledger methods
including those two skips, and nine version-line methods are represented once.

Head/tree/clean state, the original 49 captured sources/scripts/tests and
Git/Python/sh/bash entry identities match before/after. Review found two consumed
inputs omitted from that capture list: `alab/setup.sh` and `alab/versions.toml`.
Both are archived in an explicitly post-run-only supplement, byte-matched to the
tested commit with normal tracking flags and a clean working tree. They are not
called original before/after captures. With no source/test change, failure or
unresolved functional concern, independent review approved this bounded
supplement instead of duplicating the functional run.

Tests use owned local Git, module-local native responses and shell version-only
fixtures. The current pre-promote fixture stops at its committed certification
note. No real Agh/GitHub request, production native gate, Rust compilation,
shipping certificate or change to the protected compiler/profile is exercised
by these tests. The separate public Git source read is retained as such.

Final SHA-256 identities: reader
`841b663ab752a58d819c5fee8f3ab74136ba16a5e527f98d5fcbb7b9ebf5aa81`,
publisher `e2d59b79ef70a460020361094627065b263064133d74cf2d81f24cd6e12a48a8`,
ledger `435b31fdd0f8e2a2d2430fe9ee6d00052590dca1be756aceec5ec2c6b6e84d5f`.

The recursive common-Git archive is
`alab-validation/f6db0d47c044478695a0c66286e52f61788b94ea/github-integrity-c2t3ikk5`:
349 payloads, 4,069,469 bytes, 275 immutable Git bindings. Root verifies exact
inventory, every payload size/hash and the selected source bindings. It retains
all focused pairs, original proofs, the distinct eleven-method history, final
run/raw logs, controllers, primary Git source and the qualified supplement.
Index SHA-256:
`5d9b894e6aeea05bda64a10eb5b672e9f640465acda7361d97ebfbdf6f8a74ee`.
Final root log SHA-256:
`9ab710b43f7cd10d31b120efbea16b9c8318d39ff1d8cc165388ab9d56fe71bd`.

Publication also preserves remote main
`25494fb9bf001f4af8985a8e453a409accc82be7`, whose incoming changes concern
native correlation diagnostics and their documentation. All 49 captured inputs
and both post-run supplement files remain byte-identical. The test result stays
bound to `f6db0d47c044478695a0c66286e52f61788b94ea`; incoming native code is
not described as executed by these Python checks.
