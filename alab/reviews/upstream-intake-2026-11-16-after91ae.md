<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->
# Upstream intake after 91ae3d10bf

This source-only intake reviews the next twelve linear upstream commits,
`e87cd55ed96c..27e2227b2b49`, before any merge. The canonical decision record is
`alab/upstream/27e2227b2b-from-91ae3d10bf-intake.json`. No upstream commit is
integrated or qualified by this review, and every test named below is a
source-declared candidate marked **not_run**.

| commit | decision | subject | canonical patch SHA-256 |
|---|---|---|---|
| `e87cd55ed96c` | adopt | Expose context usage in statusline state | `bc88b8aa9dac06bdb74f54b31647646f9a9b14e7a12bf82038a2802af48b1fe8` |
| `5ca2007abe0f` | adopt | Expose thread costs and reject stale usage reads | `75a6e0b5d5e43da79837810353adea7bc83e3897b1018daba9cf2a54815c99e4` |
| `641e2e362ca7` | adopt | Ignore stale workspace headlines and archived messages | `a501f8edeec263dbdfb1229ebfb859afeb10f858681d4fc5d2e92f1d6fe5c300` |
| `4495f7ce1741` | adopt | Expose workspace headline in structured status state | `c9e40e8e1824bdd21421b11eeae6e4a700324dc16f848fde8f66187cb513a380` |
| `fb0715949b07` | adopt | Expose Codex rate limits in statusline state | `5b3a70423b8f71361be9c7a638b2a1da6b56fe7bdbd601010f6a4268a3c64086` |
| `ad8b7f113454` | adapt | Require a government workspace for the government backend | `b78480e748f89cb0a15fb79c5d23315a9604e3958028f9760b9f8b44d2adae96` |
| `2d5bfb7931a4` | adopt | Resolve pasted Windows paths with wslpath in WSL | `8d5d2f5a86a25290efb88f9bfbaead772b4ce4c6a07c04074daa6b5bb845055a` |
| `51f0b53d8b07` | adopt | Clear the user endpoint override during GCP setup | `f76d0e1288a158a1afcad320f5bbf1488f11a94a96e338d5a459c2701688a372` |
| `4598f3379098` | adopt | Use thread-scoped model catalogs for model lookups | `eaa319ef54e3fd3fae945647706a320b41a6541925761140bf16c24ea5163610` |
| `68c63d72d38f` | adopt | Add keyboard table selection to the transcript | `1d4c5497c03e7af487ee4e81e67d22dff2df32aeeb2112da669e1bb193117b7a` |
| `01806453b03b` | adopt | Refill the transcript viewport after tool collapse | `508905001f6d1255cb8d575ced4ea120dbe8595e91accfc8c5a5d660273997b6` |
| `27e2227b2b49` | adopt | Enable incremental tools by default for Amazon Bedrock | `4231c3c82a07c867015bfe8b209a250c9b65c9df809bc503d9b378def4fa85fe` |

## Statusline context, usage, headline and rate limits

These commits extend the same explicitly provider-facing snapshot whose reads
remain staged. The prior hold applies to every new field, not only the headline:
before any provider delivery, require explicit provider selection, current
thread/account/source ownership, and metadata-disclosure review. A project or
stale provider must not receive context, token totals, cost, headline or
rate-limit data.

`e87cd55ed96c` reuses the built-in footer's context-window calculation for the
displayed thread. Keep latest-context usage distinct from cumulative billing and
goal accounting; an unknown window omits the percentage. Preserve ALab's native
interface projection. Candidates: `statusline_context_uses_the_footer_window_and_percentage`,
`context_indicator_shows_used_tokens_when_window_unknown`, and
`statusline_state_projects_the_displayed_settings`.

`5ca2007abe0f` replaces per-widget counters with UUID identities, preventing a
replacement widget from accepting its predecessor's thread-usage result. It
also projects supported nonnegative costs as display data. Preserve ALab's
account-email recovery, quota-origin custody, service-tier and interface hooks;
never use provider-facing cost fields as billing, retry, quota or assignment
authority. In addition to upstream's replacement-widget and enterprise-plan
tests, require a direct structured-state control proving negative micros omit
both serialized cost fields.

`641e2e362ca7` applies the same identity repair to workspace headlines, rejects
stale work before fetch, and excludes archived messages. Preserve account-change
invalidation and ALab ChatWidget/app-event hooks. Upstream tests cover stale
result rejection; integration must also prove the new pre-fetch return avoids a
server request.

`4495f7ce1741` exposes the cached headline and consistently emits an account
object. Headline text remains account-scoped presentation content. Provider
reads stay staged: activation requires an explicit provider plus current
thread, account and source ownership, including MCP-bound metadata review.

`fb0715949b07` exposes distinct Codex rate-limit windows with raw reset times,
empty arrays for no windows and null for a missing reset. Keep ALab allowance
and rate-limit snapshots authoritative for native recovery. Candidates:
`rate_limit_snapshots_keep_separate_entries_per_limit_id` and the structured
state projection tests, including exclusion of non-Codex IDs.

## Government workspace routing — adapt

`ad8b7f113454` correctly refuses a commercial `NO_CONSTRAINT` workspace when
the selected backend is the government origin, but two ALab integration points
must be resolved deliberately:

- Keep upstream's dedicated `GovernmentWorkspaceRequired` match arm before the
  `ALAB(workspace-routing)` catch-all. Its public response must remain invalid
  request `-32600` with `reason: governmentWorkspaceRequired` and
  `action: relogin`; the generic ALab helper is only for remaining routing
  errors and the structured discovery timeout.
- Keep the new error fail-closed through `alab_bootstrap_account`. It is not an
  invalid-credential result and must not become an unauthenticated success. The
  TUI timeout classifier must neither retry nor redact the government response.

The upstream hostname guard also has a concrete policy gap. URL 2.5.8 and IDNA
preserve the terminal root-label dot, while the patch compares `host_str()`
literally. `gov.chatgpt.com.` is DNS-equivalent but misses the guard. During
integration, canonicalize one terminal dot for this comparison or reject such
backend hosts consistently, then cover required, effective, case/default-port
and trailing-dot forms.

Required mapping controls assert the exact `-32600` code, reason and relogin
action under ALab, bootstrap fail-closed behavior, and no TUI retry or data
loss. The existing discovery-timeout reason and legacy compatibility remain
separate.

A post-intake TUI consumer control now drives the exact government response
through the startup read. It requires one request, immediate termination,
unchanged typed code, message and data, and rendered reason and action. This
closes only the consumer no-retry/no-redaction requirement. Producer mapping,
bootstrap fail-closed behavior and trailing-dot hostname canonicalization stay
attached to upstream integration.

The expanded focused unverified test lane passes all eight
startup-account-read cases, with 6,436 unrelated TUI tests filtered out.
Scoped all-target Tippy exits zero; its warnings remain confined to unchanged
upstream sources.

## WSL path normalization and GCP setup

`2d5bfb7931a4` uses fixed `/bin/wslpath`, a one-second reaped subprocess,
absolute-result validation, and a mountinfo-gated traversal-safe fallback.
Path normalization grants no filesystem or trust authority. Run the Windows
path-with-spaces, missing-wslpath/default-mount, and completed/hanging/descendant
tmux helper controls on Linux; this Mac cannot establish live WSL behavior.

`51f0b53d8b07` clears only the active user `base_url` in the prepared GCP write
after policy composition and commits only after token verification. Managed or
higher-priority conflicts still refuse setup and auth failure preserves the old
file. Preserve ALab credential-source/network admission and unrelated provider
headers. Run the successful setup, invalid-input/credential refusal, and policy
or endpoint conflict suites.

## Thread catalogs and transcript behavior

`4598f3379098` consistently resolves model metadata through the owning thread or
destination catalog for sampling, prediction, memory, import and extension
startup. Preserve ALab provider partitions, credential epochs, originator
ownership, Guardian routing and exec handoff. Restored or imported sessions must
reconstruct the destination manager rather than carry a stale source-process
manager. Run import, parent-catalog sampling, fork/MCP, host model info and idle
classifier controls; add or require a Guardian-specific per-thread catalog
assertion because workspace binding alone does not prove model selection.

`68c63d72d38f` adds user-invoked Markdown table cell/rectangle selection through
existing transcript and clipboard paths. The free leader binding remains
remappable or unbindable and grants no automatic export. Preserve ALab's
`misalignment_review` and `safety_recourse` entries when updating generated
`core/config.schema.json`. Exercise remapping, rectangles, record headers,
Unicode/escaped pipes, empty cells, wrapping, mouse takeover and confirmed
clipboard delivery.

`01806453b03b` clamps an invalid reading anchor after disclosure collapse so the
viewport refills. Preserve selection ownership, following state, active-tool
focus, repeated-render stability, turn tips and copy feedback. Run
`collapsing_tool_group_refills_viewport` and the expanded follow/control-click
geometry coverage.

`27e2227b2b49` enables incremental tool declarations by default only after the
user selects Bedrock, matching the existing GCP policy. Explicit opt-out still
restores full declarations; collaboration child tools stay in
`additional_tools`, changed-catalog notices do not rewrite history, and
compaction opens a correct new window. Preserve ALab's feature entries and the
`alab_epoch` fixture initializer in overlapping files. Run Bedrock default and
mantle/runtime default/opt-out cases together with the GCP and Responses
controls.

## Evidence boundary

This review inspects canonical commits, full diffs, touched-path overlap and
declared tests only. It performs no merge, Rust build, runtime execution,
qualification, package publication or installation. Those claims begin only
after the ordered range is integrated with the adaptations above and the named
controls actually run.
