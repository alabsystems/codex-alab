<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Ident checkout cleanup preserves the first failure

At `f97e5f51c45e27c78eb9cfb01403f957fdb6fa7f`, a public
`validate_ident` call could replace its source-read error or KeyboardInterrupt
with a later stdout-close error. A timer-cancel error could skip child retirement
and pipe closure; setup after a returned Popen was outside the cleanup owner.
These paths already refused validation, but lost the original failure or left
resources to a later owner. Correcting ownership and error identity is warranted
without changing which source bytes are admitted.

The original six-case proof used an owned Git repository, a real expanded ident
checkout, a private comparison index and actual `git cat-file --filters` output.
Only module-local read and owned-stream close faults were controlled. Healthy,
read-only and close-only cases established the controls; both paired read/close
cases escaped the exact secondary exception. Every proof child was reaped and
its pipe closed, with no child/pipe/source-descriptor fallback. Source and both
indices remained unchanged. This does not claim spontaneous native faults or
false qualification.

Decision: keep the repair inside `_matches_checkout`. Its owner now covers
Popen assignment and subsequent setup. It preserves the exact first body/setup
BaseException, independently attempts safe timer/child/pipe cleanup stages, and
attaches guarded advisory notes for later errors. With no body failure, the
first cleanup failure propagates unchanged. Cancellation cannot bypass the
remaining cleanup. A bounded join and live-timer check prevent successful
validation with an observed inflight callback. The descriptor owner, Git/content
policy, streaming comparison and deadline policy are unchanged; root verified
the complete production file outside this function is byte-identical.

Retirement attempts are not retirement proof if native operations fail. Joining
uses a published thread ident; this is not a general quiescence guarantee for
arbitrary interrupted thread bootstrap. The inflight-timer regression explicitly
observes the callback alive when validation refuses, then its fixture releases
and joins it. Empty fallback lists in that test count child/pipe/source-FD
cleanup only, not timer release. No generic exit classifier or process lease is
introduced.

Four new public API methods exercise paired failures, healthy/single/sole-cleanup
controls, cancel-then-fail continuation, post-Popen setup failure and a real
inflight timer. Eight controls record reaped children and closed pipes, with no
child/pipe/source-FD fallback; the setup-failure child is killed and reaped rather
than required to exit zero. Actual source, attributes and both index bytes stay
unchanged.

With identical final tests, the literal old Git source baseline ran five methods:
four failed parents, five assertion reports, one passing canonical LF/CRLF
control, zero errors/skips. The sole four-module candidate run selected and ran
71 unique methods in 36.656014 seconds: 70 terminal OK results, zero failures or
errors, and the preexisting non-UTF8 filename filesystem subtest skip. All four
new methods passed. The maintained named-result wrapper retains strict exit 1
because of that skip; this selection remains nonqualifying. The skipped branch
is not claimed covered.

All 417 Python/ALab-shell/README input hashes and HEAD were stable within each
phase; only production source_content.py differs between baseline and candidate.
The saved runner records its hash, actual environment and explicit owned TMPDIR
before importing tests. Root and peer independently reconstructed ordered raw
results, old Git bytes and identical final test bytes. Three root review-script
assertions were corrected to match the recorded baseline hash, literal skip
reason and legitimate killed-child status; no test rerun replaced the evidence.

Evidence: `~/.codex/.ubd/source-checkout-primary-proof-2meopeq5/` and
`~/.codex/.ubd/source-checkout-primary-fix-qbfwgvaq/`. Candidate log SHA-256:
`8bced642874eca2a4cbabe1c7bfc4b19a8a66f0c4a34c051f57bd8ba7a7a1550`.
After both reviewers released reads, the exact proof world and old-source export
were archived with 51 entries, matching types, modes and payloads, then retired
after fresh identity and no-holder checks. Archive SHA-256:
`b8760c97fb248c67efbca7a7a7135e73c2fcaf503c5c77136f77d92c75bd14c3`.
The two explicitly recorded empty suite roots were separately checked and
removed. Raw logs, runners, result records and input snapshots remain intact;
logical retired bytes do not establish exclusive volume-space reclamation.
These are local Python/Git/process fixtures, not a certified compiler build,
native Codex runtime check, account reset or release qualification.

## Separate verification after merging main

Incoming main `09c2a831e3a95e238dd5e46803cd71e43d5ed2d4` added the full
GitHub fetch destination-policy comparison and split the filename test into
representable-content and exact raw-Git missing-leaf controls. Both changes
were independently reviewed and normally merged with this topic into
`d44d4a94c79760617e727e1b8d858b816ce075c1`. Cleanup production code and all
new cleanup methods/helpers retained their exact topic bytes.

One separate merged-source run selected and completed 122 unique methods:
72 source/qualification, 48 GitHub transport/publication, and only the two
specified updater bootstrap routing/refusal methods. All 122 recorded terminal
OK with strict exit 0, zero failures/errors/skips, in 51.681139 seconds. All 418
recorded source input hashes stayed unchanged. Actual environment, runner hash
and owned TMPDIR were recorded before imports; its fixture root ended empty
and only MainThread remained. Root and peer independently reconstructed raw
ordered IDs, result rows and source/launch bindings.

This is a distinct result, not a replacement for the original 71-method run
with its skip and strict exit 1. The raw-Git test checks exact absent-checkout
refusal; a positive physical non-UTF8 filename branch is not claimed on APFS.
No full updater lifecycle or native application qualification was executed.
Evidence: `~/.codex/.ubd/source-checkout-integration-549dj8bp/`; raw log SHA-256
`89d5fdcef5a09b53a829782da8ecfc12855d660dbad4f1ad91dc1d5f39e5abef`.
