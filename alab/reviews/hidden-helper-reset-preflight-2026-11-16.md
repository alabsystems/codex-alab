<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Keep earned-reset decisions on visible requests — 2026-11-16

Decision: apply the existing hidden-helper exclusion to proactive reset checks
as well as recovery after a quota error. An ephemeral TUI title, rename
suggestion or recap is not a conversation in which reset decisions can be shown.
Headless exec requests retain their existing reset behavior even when their
thread source is named `system` or `thread_title`; saved title-feature threads
and ephemeral side chats remain conversations.

The TUI constructs its helpers with `TemporaryThreadKind::{Title, Recap}` in
`temporary_structured_request.rs`: ephemeral feature threads named `thread_title`
or `system`, using the CLI session source. In `app_server_events.rs`, these
threads are excluded from visible routing, and their temporary request receiver
gets only item and turn completion notifications. Reset warnings emitted on
such a thread never reach the visible conversation. `recover_weekly()` already
excluded helpers, but `preflight_weekly()` did not: a helper at 99% weekly usage
with a refresh more than 48 hours away could consume an earned reset and send
its inventory and spending decision only to the hidden thread.

A shared request visibility guard now gates both entry points in ALab-owned
glue. The existing private helper regressions first exercise the real preflight
entry point at 99% with 72 hours left and one earned credit. Title and recap
helpers must perform no usage GET, credit GET or consume POST and emit no reset
notice. User threads, ephemeral side chats, saved title-feature threads and
headless exec controls must perform the three usage reads, one list and one
consume POST, with the inventory, expiry and spending notices delivered. The
same cases then exercise their original near-refresh reactive quota wall.

Rust execution is deferred while the native CPU authority refuses admission
because its slot-file identity changed. This change makes no passing Rust-test,
certified-build, package or installed-binary claim. Source review and scoped
formatting with the source-selected Trust 15698 `trustfmt` pass, as does
`git diff --check`; no upstream file changes.
