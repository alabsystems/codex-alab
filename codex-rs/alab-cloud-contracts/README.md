<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Cloud credential contracts

This crate has no production behavior. Its integration fixture uses the public
login, native source, HTTP factory and Cloud client APIs together, without
exporting private login test helpers or adding hooks to upstream files.

The parent starts an isolated test process with a generated local CA. The child
loads two actual native direct readers, refuses cross-reader factory pairing,
and completes Cloud `ListThreads` over TLS/HTTP2 with verified synthetic
authorization and account headers. An empty destination allowlist must deny
without a request while the original client remains usable. After the original
reader's native connection becomes unavailable, that same retained client must
fail without another request, and the second reader must remain usable.
The fixture awaits its server and native reader retirement before returning.

Run the parent selection, which checks the child's real exit and completed test:

```sh
targo --unverified test --locked -p codex-alab-cloud-contracts --test reader retained_cloud_transport_owns_its_actual_reader_and_destination_policy -- --nocapture
```

Use the project's admitted native controller. This is deterministic transport
qualification, not a live Cloud service or account entitlement smoke. The Cloud
connection entry point's call to the existing binding API is reviewed separately;
this fixture does not replace that entry-point review.
