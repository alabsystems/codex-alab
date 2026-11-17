<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Fractional streamed retry milliseconds — 2026-11-16

Decision: in enhanced mode, scale accepted rate-limit milliseconds to floating
seconds and use Rust's checked Duration conversion. The existing message grammar
accepts decimals, but its integer cast truncated 0.5ms to zero and 1.5ms to 1ms.
The conversion lives in private ALab-owned glue behind a marked upstream call.
The original inactive return, regex, f64 parse and seconds branch stay intact.

Only the existing rate_limit_exceeded and slow_down codes reach this fallback.
Valid nested error-header advice keeps priority, including Some(0). Nonfinite or
Duration-overflow values are rejected rather than saturated by an integer cast.
Standard nanosecond rounding follows floating scaling; intermediate floating
rounding remains, and exact original-decimal or millisecond arithmetic is not
promised. This topic adds no public API, dependency, manifest, lockfile or default.

The owned tests run parse_failed_response and real response.failed event dispatch,
then the actual API bridge. Before checking raw durations and captured deadlines,
they require the rate-limit category, original message, protocol category, no
invented HTTP status, retryability and identical copied advice. Tokio time stays
paused. Integer 28ms precedes each positive 0.5ms row, separating value loss from
setup failure. Raw conversion is checked before timer admission can hide it:
a huge valid Duration can fail Instant range admission even after a wrong cast.

Initial RED 8da891164744701ee2e8bb8b7362f551b002fbc9 executes 882 outer methods:
880 pass and two fail, zero ignored/measured/filtered. Both positives complete
28ms, then fail the late raw 0.5ms assertion at line 84 with Some(0ns) versus
Some(500µs). Later positive rows are GREEN-only. Three other new outer methods
pass; one is the exact-child dispatcher, a no-op in the outer full suite.
Two nested child failures are excluded. Suites are 93+371+279/2+137.

Main advanced from 33eb7462 to b862d9138 during the initial wait. All 115 incoming
peer paths remain byte-exact, and the selected libraries actually reran on
merged e3f5dbbdf147ee3fa5a05982afb91f97103fbd89. All 935 methods passed, but the
scoped verdict rejected one redundant_clone warning in the new fixture at line 19.
Tippy exited 0 under --cap-lints warn; that original attempt remains explicitly
unqualified. Its unit passes are not a lint pass.

The only fixture correction removes detail.clone() inside json!. The pinned
serde_json macro borrows the value, preserving both real parser inputs. Corrected
test SHA256 is fe5f753f1fefb439e6470fb4a1b17eba304bcd008f66fca9a1feceb39040744d.
Original evidence, including the lint rejection, remains under private
*-original archive paths. Both final phases use the same b862 peer base and
identical corrected test bytes.

Final RED 2d1f2d4bba5d9541c05c755c5bec45925511733d runs 935 outer methods:
933 pass, the same two positives fail, zero ignored/measured/filtered.
Suites are 93+424+279/2+137; integer-control/late 0.5ms failure context is unchanged.
Final GREEN 0c741cc51c2a7c5a61f151189fb7944d113369fd passes all 935 methods:
93+424+281+137, zero failed/ignored/measured/filtered. Five required method names
pass once. Every scoped all-target Tippy category and set-aside count is zero;
the compiler and native controller exit 0. The helper and production hook remain
byte-identical to approved 06ecc6060f0e51668cad9e930621c1f5c4a3841c.
Only this decision record follows the final Rust execution.

The completed matrix has 64 rate rows: 26 positive, 32 control and 6 inactive.
Each runs both real parser paths and the bridge; 60 raw conversions are checked
separately, with four header-priority rows skipping raw fallback. Five non-rate
inputs add 10 terminal/malformed outcomes. Controls cover zero/header priority,
invalid-header fallback, seconds, integer/fractional milliseconds, nanosecond
rounding, finite and nonfinite overflow, malformed fields and live vanilla mode.
Successful child output is drained; row reach follows each passing wrapper and
its loop control flow, rather than emitted row markers.

Both final phases use source-selected Trust 15698 and the native runner in the
explicit unverified test lane. Independent observations retain all four live
binary hashes, preparation snapshots, execution receipts and diagnostic identities
for each phase. The original attempts are retained separately. Incoming CLI
status/collector changes preserve the result/receipt schema; five focused fixture
suites pass 2+10+10+10+14, each actual exit 0 recorded. These use controlled failures
and temporary Git fixtures, without network or package-promotion qualification.

Bounded static audits found no concrete malformed request in routing-state reset,
fallback, prefix or request preparation, and no stale recovery-guide link, schema
column, label or read command. No snippets or live database reads were executed.
The construction audit does not establish full provider-schema validity or
supply missing incident wire data and backend causality.

Evidence covers source/library arithmetic and scoped lint. Physical wire timing,
host sleep and actual Core/Autopilot retry execution are unmeasured. The original
opaque HTTP 400 backend cause remains unknown. Application certification retains
its recorded source-selected prerequisite; no package or running binary is
replaced. The recovered pre-existing tab keeps its task.
