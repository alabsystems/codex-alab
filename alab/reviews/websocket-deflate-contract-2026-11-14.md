<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# Preserve negotiated compression through the rejection adapter

The public Responses endpoint enables permessage-deflate. Existing configuration
tests establish that it is offered, and the rejection adapter's successful-101
controls establish ordinary frames and buffered tails. Neither proves that an
actual negotiated compressed model exchange retains the original codec state
through the new stream wrapper.

## Decision and reason

Add a contract in the existing ALab API test crate, using the actual public
Responses connection and the pinned ordinary Tungstenite peer. Run both a
peer that negotiates compression and one that declines the offer. Each case
performs two typed requests over one accepted socket, with a previous response
reference on the second request. Require exact Unicode input and output,
response IDs, completion and token usage.

A bounded transparent byte tap records only real successful reads and accepted
write prefixes, including vectored writes. The existing HTTP and raw-frame
parsers verify the actual offer, selected extension, client/server masking and
RSV1 on the text-message starts. This prevents a silently uncompressed exchange
from satisfying the compression control. The peer uses the original codec and
verifier; no decompressor or protocol implementation is copied.

Exact child processes isolate proxy and CA environment without parent mutation.
Accept, handshake, model sends/receives, completion, child lifetime and cleanup
are bounded. Owned tasks abort on drop; child deadline cleanup retains the
handle for termination and reaping.

No production defect was found in the hostile review, so this changes only
coverage. It tests wrapper interoperability rather than independent DEFLATE
conformance. Partial and vectored I/O are supported but not forced. Two exchanges
with context takeover permitted do not independently prove that the second
payload requires the first dictionary. Existing buffered-tail tests remain
complementary; this fixture does not force coalescing or change flush semantics.

## Validation

Checked source `dacb8db2892b60449ff3454f22fcc7c89fd0e637` passes all 14 API
contracts, including both new compression cases, in 0.45 seconds. Source-selected
Trust 15698 executes them in the explicit unverified test lane. Scoped all-target
Tippy for the API contract crate reports no findings, and committed ownership
validates all 86 hook topics across 13 features. Independent source review
accepted the fixture and its public API compatibility. No production files or
dependency identities changed.

The portable workflow subset separately passed 146 tests at the preceding clean
source `ee3dd83770a71b80528abca9fe7e775b094139e7`: no failures, errors or skips,
in 74.713 seconds. That is a subset observation, not a full-suite result.

The complete configured workflow suite requires the documented real native test
adapter. Its supporting build was queued for over twelve minutes without its
16 GiB memory admission, behind existing VM usage and older native requests.
The agent terminated only its own queued admission request; the native front
confirmed that compilation did not start, no artifact bytes were produced, and
the pipeline returned 143. The preceding API tests and Tippy had completed
successfully. The full workflow suite was not started; it is neither passed nor
failed by this observation. Keep its native prerequisites and timeouts intact,
and resume with the real adapter when admission is available. No owned commands
remain running. Logs, source identities, hashes, the independent review and
the explicit deferral are retained under the common Git directory's
`alab-validation` records.

Certified application delivery remains separately deferred at the recorded
[Targo macro-admission boundary](targo-macro-admission-2026-11-14.md).
