<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Streamed misalignment details — 2026-11-16–17

## Decision and reason

Add public SSE and real WebSocket coverage for nonempty misalignment continuation
fields across diagnostic wrapping and API error mapping. Existing public fixtures
checked absent details; upstream separately checked parser output and constructed
bridge inputs. A mapper could lose these fields while retaining the displayed
message and terminal category. The current production bridge correctly forwards
them: this is GREEN-only coverage, not a reproduced production defect.

The two existing ALab fixture modules gain one outer method each. They reuse the
existing transport, isolated child, context, safe-ID, policy and bounded cleanup
checks. Each method declares a complete-details row and a malformed-details row,
four declared rows in total. These are source-loop counts, not independent wire
or frame measurements. Existing no-details cases and outer case lists remain.

The complete named-object response.failed payload carries four distinct literal
fields: error_type, detailed_explanation, steer.message and review_target. The
opaque target includes leading/trailing spaces and punctuation. Both the original
ApiError and mapped CodexErrorDetails must preserve every field exactly; the new
details oracle does not run its expected value through the mapper under test.
A numeric steer.message instead drops the malformed details while retaining the
terminal refusal. Contradictory Retry-After advice grants neither retry nor
transport-fallback authority. INFO diagnostic assertions exclude every detail
field and private sentinel while preserving only the expected safe IDs/context.
No standalone WebSocket error-envelope admission or production parser changes.

## Initial qualification

Test-only source 09f667926d7345d0ed7d26167b13f131aee7af72 changes only the two
owned Rust test modules. The maintained full-library run passes 105 public
contract methods and 283 API methods, totaling 388 outer methods. Failed,
ignored, measured and filtered counts are zero; both new methods pass once.
Scoped all-target Tippy has zero owned, spanning, failing, missing-script or
set-aside findings. Unit, lint, controller, summary and capture statuses are zero.

The qualified frontend selects committed Trust 15698. Tests use its explicit
unverified library lane, one ordinary record round and one test thread. Source
and target stay fixed through live observation and independent qualification.
Two library images and the native runner, launch receipts, CWD, diagnostic
identities, compiler pin, both lint profiles and 657 canonical artifact paths
are verified. Observation and independent qualification are separate records.
No application certification is inferred from these library results.

## Composed-source qualification

Remote main advances to 9ee12e330bb9a41a1355aa8772b9808e4342b804, changing 19
paths including AGENTS.md, lib.sh and command_guard.py. Review accepts preserving
those already-main bytes but requires a fresh result because executed helper
inputs changed. Merge source 053c6379a9b17b66bc52df3bfd0c04ef97176277 preserves
both owned test files and all peer files byte-for-byte.

Its separate composed-9ee phase waits for the existing exclusive memory claim,
then freshly passes the same complete 105+283=388 methods with all other counts
zero. Scoped Tippy, summary and capture again succeed with zero findings.
Independent qualification verifies the new capture, current three images,
receipts, diagnostic identities and all 657 lint artifact paths before releasing
the hold for this decision record alone. Initial results remain historical;
counts are not added across phases. Closing controls require the explicit
composed phase and source, so initial evidence cannot substitute for it.

## Publication and limits

The private misalignment-details-20261116 stage retains source-bound raw results,
initial native exits before post-validation, captures, reviews and control hashes.
Publish through the maintained expected-main guard, preserve disjoint peer work,
and retain the composed runtime attribution through publication and cleanup.

This tests existing error-data custody and INFO diagnostic privacy. It does not
qualify Core continuation actions, a real provider, every logging level or an
installed application. The original opaque HTTP400 cause remains unknown.
The source-selected certified application prerequisite and human hold remain;
new gate attribution rules do not establish a compiler capability change.
