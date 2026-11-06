<!-- Copyright 2026 Andrew Yates -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Authentication recovery during startup

The account read is both the TUI's login decision and its initial account display.
It must remain available when saved ChatGPT credentials are rejected. Previously,
workspace discovery exhausted reload/refresh and returned a JSON-RPC internal error;
the TUI exited before it could offer sign-in. A revoked access token can still have
an expiry days in the future, so checking expiry or file presence cannot fix this.

`alab-core/glue/bootstrap_account.rs` maps only exhausted authentication failures
(`DiscoveryUnauthorized` and `RefreshTokenRejected`) to the protocol's signed-out
account response. It neither deletes shared credentials nor manufactures routing.
The login screen then uses the normal account login API. All model sessions still
use the strict routing resolver, and configuration errors, policy conflicts,
account changes, and non-authentication discovery failures retain their errors.

The auth audit covered TUI login/bootstrap, background routing, model routing,
login completion, workspace switching, and provider auth status. Login completion
already reports failed discovery inline; model routing and workspace switching
must continue to reject unverified routing. The recovery change belongs in the
account display API so all clients can reach sign-in, rather than in error-string
matching in one TUI. Permanent refresh rejection and transient refresh failure
are both regression cases, including a failed model turn and successful subsequent
login. Existing routing tests cover account changes and routing constraints.

A separate fleet prerequisite is renewable account metadata: a default login can
exist while its aterm roster row lacks an email, and a fresh machine can lack a
headless browser. Such a row cannot renew automatically. Record the existing login's
identity through the account-management path and provision its supported browser;
do not copy another account's tokens or reinterpret rejection as successful auth.

## Release verification (2026-11-03)

Build `2026110301`, published through signed index `79`, contains source
`ef71b0aa49fca48946749b43245f6f7eb17f51de` and reports
`0.1.0-dev.14414+codex.0.166.0.gef71b0aa4`. The full `alab/check.sh` gate passed
on that source, including the ALab tests, lint attribution, and all 29 selected
upstream library test binaries. Separately, all 15 routing integration tests and
74 account integration tests passed; the account suite needed a 16 MiB test-thread
stack for its existing large enterprise-auth futures.

The staged release passed manifest-signature, archive-digest, installed-tree, and
linked-command version checks before publication. On the affected fleet Mac,
the managed update installed this build, `aterm pkg verify codex` passed, and
`codex --no-alt-screen` reached the sign-in screen with the rejected saved token
still present. The startup recovery is verified; a replacement authenticated
session was not claimed. Automated renewal stopped at OpenAI's Cloudflare check.
