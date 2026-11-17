<!--
Copyright 2026 Andrew Yates
SPDX-License-Identifier: Apache-2.0
-->
# Accepted connection resource custody

The retained input previously handed its transport only a Unix stream. A later
request could not retain the concrete process-local resource associated with that
same accepted endpoint. This component carries an optional resource through the
existing WebSocket connection event, connection session and original request
context. It adds no listener, queue, runtime or permission decision.

`IncomingStreamProvider::accept` now returns one `AcceptedConnection`. The final
RPC acceptor consumes the envelope. The preparation protocol explicitly consumes
only its stream and drops the resource; it never reconstructs native origin.
Ordinary Unix, WebSocket, stdio and remote-control constructors supply no resource.
The original connection origin, authentication and RPC gate remain unchanged.

`ConnectionResource` retains an actual process-local allocation. Its presence,
type, downcast, request ID and connection ID do not authorize anything. A future
concrete native consumer must use the real resource's SDK currentness checker.
The existing proof-only native provider intentionally remains stream-only.

The new controls use real Unix endpoints and WebSocket framing: cancellation
preserves the same queued socket and resource; preparation drops the resource
while the same stream works; and the same resource reaches the real allocated
WebSocket connection event and connection ID. Existing request/runtime, task and
startup controls cover the changed constructors. These component tests do not
qualify a native SDK handoff or prove resource delivery to a protected operation.

Validation used the source-selected Trust compiler in the existing protected
project runner. Input `019eb809792670b1efd015aa909720a2f6cb396e5aaf36730c58bf870dc16385`
on `40fbb7356b46c503af71328fca6f3aa657ed7a8e` ran 6 core, 175 transport, 31 retained
request, 38 task, 2 bootstrap, 8 preparation and 278 aterm adapter entries. There
were 529 passes, 5 failures and 4 ignored subprocess entry helpers. Those four
helpers are driven by their actual parent tests, not counted as separate passes.

The 5 transport failures are existing local daemon socket controls returning
`Operation not permitted` in the unchanged socket setup before the changed callsite.
The protected profile denies writes to the fixed shared daemon directory; it was
not relaxed. A pristine same-profile runtime comparison was not run, so these
failures remain visible and are not called qualified baseline failures. The new
real WebSocket resource identity control passed.

Five-package production/test Tippy found 0 ALab-owned, spanning, failing or
missing-script diagnostics (11 upstream diagnostics set aside). A manual audit
of warnings in changed upstream paths found only 3 duplicated, unchanged original
ClientRequest patterns. The explicit native-source-proof library check and
feature Tippy exposed one unused import. Removing only that import produced
final input `fcb44960816f12a8528509ca276c7b7d259d27847283d3679b0a32134ba8e524`;
explicit feature check and feature Tippy then passed with 0 diagnostics. The other
22 file hashes remain the actual runtime-tested inputs; runtime results are not
relabeled as a clean-commit build.

Exact logs, terminal statuses and manifests are preserved in the workspace
record `source-connection-envelope-validation.json`. Independent bounded review
found no current carrier blocker. No real protected RPC/SDK permission positive
or native C1-to-new-image-C2 qualification is claimed.
The live-update goal remains incomplete. Ordinary activation, the actual SDK/UI
replacement and a certified application build remain required.

Checked final code hashes (SHA256):

| Path | SHA256 |
| --- | --- |
| `codex-rs/alab-aterm/src/frontend_retained_proof.rs` | `9d466d1a312e589cb0384d7c62922ea2b3ccef3dd6cd6132affa72b0f4caa5fc` |
| `codex-rs/alab-core/glue/connection_resource_context.rs` | `1e542fc88e50599d1c25c3c5e92612ec37cd2365f3e074925212eca49ca7ecbe` |
| `codex-rs/alab-core/glue/native_incoming_resource_tests.rs` | `540ea92601a1c177ab46a103c105547c9dbd0076c24690c02b07cebf8fb888b4` |
| `codex-rs/alab-core/glue/native_incoming_transport.rs` | `8175e5a2e8a7b04d4ce1cd18ade9bdee285fc1e8bf941b7fa3a299cad9fb4238` |
| `codex-rs/alab-core/glue/native_incoming_transport_tests.rs` | `54e12967ad4473d58c9f805b1db24967dda39b41f7cdea22e51bb331306bc462` |
| `codex-rs/alab-core/glue/owned_local_runtime_dispatch.rs` | `a44585e1fa6399f3857dc65a5aec820943172db6dc90a54a25ee1730c1ac76de` |
| `codex-rs/alab-core/glue/owned_local_runtime_dispatch_tests.rs` | `bc68a623657be3540c8c12271685cf7e36706c77712e5dae88ef2b971db28ef2` |
| `codex-rs/alab-core/src/connection_resource.rs` | `70f256911713d565d016347b3ee8402dc3e58e9917481971d4164e4d73585256` |
| `codex-rs/alab-core/src/lib.rs` | `6bf6edd5d7fd0b0e9e648de484e56cdf7beca89c1e19e598bbf684fdcff97961` |
| `codex-rs/alab-core/src/native_incoming.rs` | `9c1b8c933ec8399d9ebcd5343913d2d26ccf1fb3f4f6373c57b64ca09ed6568a` |
| `codex-rs/alab-core/src/native_incoming_envelope_tests.rs` | `63193dfd3253aea2d0090521c73691224f52f72e5689e0ae914470759bfddfe0` |
| `codex-rs/alab-core/src/native_incoming_tests.rs` | `f6959088a3c107cfdd0617170ad45487003f9fc780f44c835dd2387ed5186680` |
| `codex-rs/alab-tui/glue/owned_local_bootstrap_tests.rs` | `e51809151661ae90a2daf463eff4ab79ad6976742fef227054a8d2a5aa0571a8` |
| `codex-rs/alab-tui/glue/owned_local_prepare_tests.rs` | `9d5eb7b9d8be9f284ccfb11aad5d297ac6e4d7ade19dbf671efe5b9920b8165d` |
| `codex-rs/app-server-transport/src/transport/mod.rs` | `5ed4e2ad1a3f9f9fa01d4894f826ce4873ab1c4bb17ce896ec8c402900c97cdb` |
| `codex-rs/app-server-transport/src/transport/remote_control/client_tracker.rs` | `13f43d9c79f811da0929dfd4a5638409ceb73137c38c07aeda52df7ef77622e4` |
| `codex-rs/app-server-transport/src/transport/stdio.rs` | `8eb700f534cd448b53665b2a875ed4f5154bb2b98d4bbdce525e85d205eea705` |
| `codex-rs/app-server-transport/src/transport/unix_socket.rs` | `43a9a71bcac4840ac0a452efef4abaaf588a6346c54aef9fae12cba7e6083b04` |
| `codex-rs/app-server-transport/src/transport/websocket.rs` | `75115dddf28b474a5db5ae93fb7523249008a2ca90d6eb59be9d7bc08e6becb0` |
| `codex-rs/app-server/src/lib.rs` | `4bae9ee43c903cb33da150869ea8076c794c113424f8e764d2119f593c008313` |
| `codex-rs/app-server/src/message_processor.rs` | `785d5eea542a80b8c9b373d0bcc46993bf80eb9b230a8109fd9330958771d760` |
| `codex-rs/app-server/src/outgoing_message.rs` | `bf8516c02c02e5795bd363e41d967fbab56f985ef3d17555292d3cc5728c3450` |
| `codex-rs/app-server/src/transport.rs` | `4549df198d6654516c46121e0c4f8f36e0611b5947d7c85cffaee447abba3221` |
