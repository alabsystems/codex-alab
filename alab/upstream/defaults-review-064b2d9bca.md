# Default activation review — target `064b2d9bca`

Partial semantic review; no qualification or all-commit approval.
Source declaration delta is saved in defaults-review-064b2d9bca.json beside
this file. It is a review aid, not a runtime configuration export. Compared
3026a8e54e0f48c4d8074cd700f35b0697abc329 to `064b2d9bca2b5ba2f9f399ffefd06a75d97852f0`.

Adopt these upstream promotions without adding ALab default overrides:
- untrusted_as_tool_output (08ae6151226a): browser/goal additional context uses
  agent tool outputs. Preserve explicit opt-out and context deduplication; verify
  ALab goal continuation/recovery under this representation.
- code_mode_host_grpc and code_mode_host_output_truncation (247934cfe5f5): retain
  upstream default transport/output budgeting and explicit opt-outs. Check output
  budgets, truncation notices and wait termination; do not disable a new transport
  merely to hide a source or native qualification failure.
- retain_client_tool_outputs (4477dcddf856): keep upstream retained evidence and
  its bounds, including across compaction and ALab native memory transfer. Review
  existing retained_context capture rather than duplicate output storage.
- unified_image_budget (0a944f8ce2b4): use the unified budget for request/compaction
  eligibility, including ALab foreground recovery. Do not restore old request
  admission through an overly broad retry policy.
- guardian_include_additional_context (6f53fbfb5b7f,a47a51892818): retain the
  selected review-evidence setting. The ALab model-context snapshot now carries
  this bool rather than defaulting it during exec handoff.

Keep new disabled features opt-in: cli_resume_account_check, tui_workspace,
hosted_search_policy, agent_message_board_mcp, mcp_lazy_stdio_startup,
command_sessions_world_state, incremental_tools_legacy,
image_generation_animation, guardian_denial_reconsideration,
gateway_setup_wizard, async_compaction_root_only. They remain carried in source;
this is no decision to delete their implementation. Their individual semantics
still need full-range intake review. Neither feature names nor textual defaults
prove consent, background effects or ALab parity.

plugin_discovery_prefetch changes true->false as a removed compatibility flag;
verify actual plugin discovery paths rather than treating this numeric change as
proof that all discovery network work stopped. ALab's existing plugins=false
policy remains separately owned.

Uploaded audio (f1b0721c8800) is gated by actual model capability, not a new global
flag. Preserve rejection attribution, durable rejected IDs and trusted durations.
ALab exec context now carries rejected IDs and duration metadata; qualification
must exercise audio follow-up/compaction/resume/model switch and ambiguous errors.

None of these feature keys appears in ALab's packaged defaults at this review.
Explicit user/profile/project/managed configuration can still change effective
activation, so runtime checks remain required. All listed scenario evidence is
pending on the full candidate.
