<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Resumed-turn WebSocket fallback on OpenAI and ALab 0.170

The official OpenAI 0.170.0 CLI and the ALab tagged candidate at
`019cbd9f6b2b136d0de1a06d8fe8c61b9fe2ffdb` chose the same transport in all
three cases. Each CLI used `codex exec` to create a conversation, then a new
`codex exec resume <thread-id>` process continued it. A loopback Responses
provider closed the first generation on the resumed turn.

| What the provider sent | Resumed generation attempts | HTTP POSTs |
| --- | --- | ---: |
| WebSocket close 1009, before any response event | WebSocket, then HTTP | 1 |
| Ordinary close 1001, before any response event | WebSocket, then a fresh WebSocket | 0 |
| `response.created`, then close 1009 | WebSocket, then a fresh WebSocket | 0 |

The HTTP fallback posted to `/v1/responses`. Each retry preserved the complete
ordered input, including the original question and answer, local environment,
developer messages and follow-up question. The first generation referenced
an acknowledged WebSocket prewarm. The retry resent those input items in full.
Both processes for each case exited zero and reported the same conversation ID.

The observer at `88f274f3d8a9cae53b7291234d17ce8a9dea9923` used Python
3.14 and `websockets` 16.1.1. The official executable SHA-256 was
`9248cfeebec9b4cc3034d5114caab10fbf426cbfa79dba410c5fa1b72762abd6`.
The ALab executable SHA-256 was
`48907dabb5b1c1aa463b00bb67e3e179cf847365e2cf948c61ead82941c933ac`.
The comparison record has SHA-256
`ff6b919804a49f5181d7f850426fb8f08652bc93331b0dda87c3122959306f8e`.
Across the six saved cases, the verifier rejected all 30 changes to the
transport, request path, warmup connection, developer input or history order.
That verification record has SHA-256
`251814cd7ed62b45b0e1c8ea2b407ae252a341d6515f786907faf82b576f6a6b`.

Run from the checkout using a Python with `websockets` installed. Supply an
unused output directory and the two existing executables.

```sh
python3 alab/smoke/websocket_close_parity.py \
  --vendor /path/to/official/bin/codex \
  --alab /path/to/review/bin/codex \
  --output /path/to/new/close-comparison
```

This measures client behavior after a controlled close frame using a small
request. A live provider’s size threshold, proxy behavior and entitlements
remain unmeasured. The existing 18 drop-in comparisons and release scenario
counts are independent. The source test for these close boundaries is
identical on the official tag and candidate (Git blob
`f963162da74fec7db4f5514e7e39c085ed1a2893`).
[Official 0.170.0 close-frame tests](https://github.com/openai/codex/blob/29eb79e9866b3262a777fdf8195c1b9e63684c91/codex-rs/core/tests/suite/websocket_fallback_close.rs)
