<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Bind matrix result hashes to the verdict input

Decision: capture each recognized result file once, decode that byte sequence,
and compute its recorded SHA-256 from the same bytes. The previous reader
parsed `read_text()` and later reopened the path for its hash. A replacement
between those observations could make a passing qualification authenticate
different bytes from those used by its predicate.

The adjacent TODO scan found no unheld item. GitHub issue listing was unavailable
with a 403 response, so source inspection supplied the next concrete issue;
no alternative permission route or routine human request was used. Existing
native qualification and producer/adoption holds remain respected.

On starting main `26b87abc2651878fbd045ed84f6928a49ed722b6`, the actual finite
public-main observation completed 0 (tool completion `c981da`). An owned file
contained passing record A. Its first real text read returned A, then the
control physically replaced the file with failing record B. The real parser
and predicate accepted A, while the later real byte read hashed B. Main
returned 0 and wrote both qualification/case `passed: true`; its recorded hash
matched the retained B whose real predicate returns false.

A's original SHA-256 is
`e685c2345b32b939144f8fc11c8580efd61031de14a7cf1520424fda7620b883`;
B and the old recorded SHA-256 are
`095e6d3c2c56ad356cf07f5af71cecff09884f10c3659faf6bb98aa9aa913a53`.
A deliberately contains CRLF, literal UTF-8 and noncanonical whitespace. Its
normalized text hash also differs, so hashing normalized text or JSON
reserialization would not preserve the observed input. The private packet is
`codex-matrix-result-byte-binding-proof-1cb8i9cz`; its nine production inputs,
source identity, Python and driver maps stayed unchanged before and after.
Its sealed index is
`4df073510d3069e5df7f5a9397752d22380f0c9cd81d36aa762f878405b83d35`,
with 28 payloads totaling 409,017 bytes and nine exact Git source bindings.
Admission and controller invocation used explicit inert seams. No native host,
Codex application, provider or compiler was launched. This demonstrates a
controlled reader defect, not an observed production writer race.

The repair keeps the existing `is_file` gate and failure/case-predicate rules.
An in-memory `TextIOWrapper` uses the same default decoding and universal
newline policy as the previous text reader. Parsing byte strings directly
would silently accept UTF-8 BOM or UTF-16/32 input; the selected implementation
retains their old refusal. The digest uses the original immutable bytes,
including original line endings. Every present parsed JSON value receives its
real hash, including `null`, `false`, zero, empty string, list and object. A
missing or unrecognized result file alone retains a null digest.

Seven regression methods were frozen before the repair, test source SHA-256
`b80f7df649a7fdf5443778f78b24b0744f23d5bf3f5b7c7d5f28757cfa0591ad`.
The test-only source `3cde208201a9b949648bbb4e3a2fa2712616404f` genuinely fails:
original tool session 56772/completion 19119b exits 1, four methods pass, two
fail and one has failing subtests. The failures cover both rewrite directions
and all six present falsey JSON forms. Healthy, exact-byte, absent-file and
decoding-refusal controls pass. The maintained packet
`codex-wide-composition-python-dv_45i40` retains 917 Git-bound inputs and stable
tools, index `7b6cb2dbc15a4e6ad85d2aaec5644725592a8dccd3405f11147def69392ecc00`.

After the repair at `e44df1fbd028c42a8749bfd5a577707be34420dd`, original tool
session 97177/completion 262f80 exits 0: all seven new methods, nine existing
strict-status methods and 26 rotation-controller methods pass. Original 40917/
completion 9156f2 exits 0 on system Python 3.9.6: the same sixteen matrix methods
pass. There are no skips, failures or errors. These are 42 distinct affected
methods on managed Python and sixteen compatibility executions, not 58 distinct
scenarios. All 917 captured inputs and tool maps match before and after each run.

The managed packet is `codex-wide-composition-python-_jj8l79h`, index
`4f1fb9314a1677064083f0fdfa5eb68363038343563d2d9775d9230d97c8d09e`;
the compatibility packet is `codex-wide-composition-python-9cq8gz_x`, index
`0370698d5b5278d77d86cb7cd97b9e8b01206247e576c0410316a96198d97a0f`.
Independent source review approves the exact repair and frozen tests.
An independent packet audit verifies all three indices, 917 Git source
bindings per capture, stable executable maps, eight genuine baseline assertion
failures without test errors, and the 42/16 successful execution scopes.

Main advanced to `d29e1f4de2b15d82de004ce6bb8c43eca8eb5b90` during review.
A normal merge produced `325a984c23a33f5925bcdeb25b0e868cd8bc1de1`, retaining
the concurrent Rust authentication fix and intake decisions. Within `alab`,
`publish`, `trust.lock` and `VERSION`, only documentation and intake metadata
differ from the tested e44 source. Executable Python inputs and the frozen
tests are unchanged. The recorded results retain their exact source scopes;
this merge supplies no new Rust or application-certification claim.

Before publication, main's shared-runner child-cleanup change at
`546e672356bc8eedaa2ebee7bd9e5e3136513acc` was merged normally into
`6fb5c71133ac963f0b1d7a366ec162134bf57529`. The matrix repair and frozen
tests are unchanged. Because the imported runner changed, the affected checks
were repeated with its new cleanup and existing signal-ownership controls.
Original 26359/completion 2ba568 exits 0: 60 distinct managed-Python methods,
five successful runner children. Original 54329/completion d20ed7 exits 0:
34 overlapping compatibility methods on Python 3.9, four successful children.
Neither selection has skips, failures or errors. Both preserve 922 Git input
bindings and stable tool maps. The packets are
`codex-wide-composition-python-lgx027u2`, index
`91b7439ccd5f52c2a0f0e04703482163376e8302869b6981d57281897e0b796f`, and
`codex-wide-composition-python-ab2pzc5v`, index
`ac10dbc828dfc0ea47faa2f35ae4dca3caf6018bc9a3fecb591ad62f975e3ac9`.
These remain selected Python results, separately from native qualification.

This change binds the digest to the input of the verdict. It does not establish
an atomic filesystem snapshot or continued custody of the mutable pathname;
a subsequent replacement can instead be detected against the captured hash.
Image hashing, resource limits and native qualification are unchanged. No new
application certificate, native run or complete-workflow result is inferred
from these controlled reader checks.
