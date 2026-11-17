<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Named receipt cleanup preserves the selected failure

Decision: fix the next concrete ownership gap beside the receipt-custody work.
The local owned-code TODO scan found no actionable marker; its literal matches
were vendored TODOs or fixture/documentation vocabulary. The issue-list read was
unavailable under the existing GitHub permission scope. That operation was
recorded and deferred without changing identity, credentials or permissions.

On starting main `19e8121509c556fdd0c7fad0093976977324f6e6`, named receipt
cleanup caught only OSError. A secondary cancellation could replace a pending
failure and abandon identity-checked unlink and later descriptor closes.
Formatting a cleanup exception or invoking its primary's optional note hook
could interrupt cleanup too. The anonymous receipt already had the needed
failure-selection policy.

Test-only commit `50b8aa1af36b37fa7bb15b30b71087d6a4ab14a0` adds eleven
public-API methods without changing production. Actual RED 10652/1e262e returns
one, retained by its typed guardian and fixture status; source status is zero.
All eleven method IDs finish: four pass and seven fail by subtest assertion,
with 180 natural assertion events, zero errors and zero skips. Eight physical
cases observe only truncation, both owned descriptors still live, and the
receipt path still present before independent compensating cleanup. Seven lose
primary identity; the ordinary-primary/cleanup-interrupt case selects the
cleanup interrupt but lacks the required explicit cause. All ten maintained
main-function cases fail their cleanup assertion first, so RED establishes no
main-return or OS CLI success status for those cases.

The failed evidence is preserved in the finite
`named-receipt-red-private-g_zp8mmt` seal, index
`6c7c996788dc17718b80a87093cc10994f85bfddf228050832137e023a081c7d`.
Its 12,332 payloads include 12,311 immutable Git bindings. Separate custody
verification checked every indexed hash/size, exact inventory and read-only
modes. No compiled binaries or Cargo target directories were copied.

Only after RED closed, commit `4c876843e48260470e7592665811e977a20bb053`
extracts the anonymous selection and safe fixed-note logic into one private
helper. The extracted selection and note statement ASTs are unchanged. Both publication success paths are unchanged,
including the anonymous success descriptor's intentional process lifetime.
Named cleanup now records BaseException failures independently from truncation,
identity-stat/unlink and each remaining close. It completes every reachable
cleanup attempt before selecting the primary and attaching advisory notes.
An existing non-Exception BaseException remains primary; an ordinary Exception permits the
first supported cleanup cancellation, raised from the original primary.
Success-shaped secondary exits and unsupported foreign exception metadata do
not gain that authority. Fixed notes evaluate no exception repr or str and
cannot replace the selected error. Identity-gated unlink, absent-file handling
and consumed-publication-close ownership transfers remain; no close is retried.

The same frozen test bytes
`81b5a6dd193cc298ea1abc68d1e9646dab713e26bae13796af222f021bac7639`
then pass with the implementation. Actual GREEN 88812/0e972d runs twelve full
modules: all twelve actual child statuses are zero and all 136 distinct
maintained method IDs finish and pass, without errors or skips. The new module
includes twelve actual owned-inode cases and ten maintained main-function
controls. Its synthetic descriptor ledger proves attempts and ordering; it does
not prove physical retirement. Physical assertions precede compensation.
Existing selected guardian modules supply their own real Python command,
signal, lifeline, observer and retirement controls. None is a native application
or protected producer qualification.

The original GREEN collector returns one because it incorrectly requires empty
direct stderr. An existing inherited-timeout test deliberately emits exactly
72 bytes from its child: `alab: network command exceeded its 120s deadline; transaction must stop`
followed by newline. The raw SHA-256 is
`55105138b50da81b3292188acf62dc27e6f2d6df492199dea00d707589d6f734`.
The source/tool/script bookends still agree,
`766e36c877df5b8b0ec68102f7979067abe867d8f832e19f62675f6843168e1e`.
All original observations, the collector refusal, fixture/typed status one and
source status zero remain unchanged. The decision is to validate the closed
results separately, allowing only those exact bytes in that exact module and
requiring empty direct stderr elsewhere. Repeating the passed tests would not
resolve a collector-only channel assumption.

Existing human holds on full application certification, Trust adoption,
protected external producer work, release validation and the ignored Land
measurement stand. These are focused Python source and resource observations;
no native build, installed activation or full application gate result is claimed.

The separate closed-data validator, actual 93939/39ce0e, returns zero with its
own typed guardian zero. It checks all 136 actual passing rows and twelve child
statuses against the frozen inventory, all 12,311 copied Git blobs, unchanged
original input hashes and the exact single-module stderr allowance. It writes
only a new derived observation. That observation is SHA-256
`00ce54a3126eb9576e5dab202435fb7fab12bacaef9f4edc330b035352a439be`;
original collector status one remains separate from this validation status zero.
No test or fixture was imported or replayed by that validator.

A fresh admitted main `3cd0f624e8a610d9ac11afc444c4ab4c87b16d4c` adds only
two upstream-intake record files. It is normally merged at
`2f84a540975e53efa9f88e7efe884dac66c3653e`; checked Python sources, selected
tests, tooling and Trust pin are unchanged, so no test repetition is needed for
that metadata alignment. Closed evidence archival can proceed from the copied
source and immutable Git objects while publication proceeds. Independent source,
test and report reviews approve the bounded change.
