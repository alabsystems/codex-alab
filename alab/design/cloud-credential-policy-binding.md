<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Cloud credential and transport binding

Audit of upstream `627b579219`: `app-server-client/src/cloud_backend/auth.rs`
captures the effective local HTTP factory and its current-account policy, then
loads a credential through `AuthConfig::load_auth`. It extracts the token and
constructs `cloud::Credentials`, which contains headers without credential source
provenance. Those two independent captures must be bound before extraction.

The direct configuration reader already traverses ALab's constructor authority
check. Its returned credential carries an immutable reader snapshot. That alone
does not place the snapshot's reader/source lifetime on the Cloud transport.
`cloud::Client` retains a route-aware client pool built from the supplied factory;
the factory therefore needs to retain the credential restrictions, including
refusal after reader retirement.

Use the existing owned `CodexAuth::alab_bind_http_client_factory` implementation
in `alab-core/glue/auth_epoch_snapshot.rs`. Immediately after loading the auth,
bind the captured factory and propagate failure before extracting its token.
The method intersects the credential's captured policy with the caller's policy
and rejects incompatible account/source bindings. It never refreshes an old
credential's policy from the current account. No additional Cloud auth store,
transport, cache or refresh path is needed.

The call-site hook is implemented in `52d280cf2a` and composed with upstream
`627b579219` in `ca525ab4d8`.
Verify the actual call site, same-source success, incompatible pairing refusal,
retained transport refusal after retirement and unchanged destination denial.
Existing login snapshot tests cover several local pairing controls; they do not
by themselves prove an actual Cloud client follows native reader retirement.
Cloud's existing launch gates and unsupported-account checks remain applicable.

The owned cross-crate fixture in `codex-rs/alab-cloud-contracts` passed on
`b36e7931d9`: real native direct-reader loading, incompatible reader/factory
pairing refusal, Cloud ListThreads over local TLS/HTTP2, preserved destination
denial, retained transport refusal after reader EOF, and both exact captured
retirement acknowledgements. Its Tippy check also passed. The receipt is
`alab/upstream/627b579219-b36-cloud-reader-qualification.json`. The fixture uses
the public binding API; the production connection entry point is reviewed
separately. Live Cloud service access, whole-source qualification and installed
adoption are not established by this fixture.
