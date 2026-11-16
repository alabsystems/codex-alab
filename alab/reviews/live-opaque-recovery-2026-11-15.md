<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Recover the live session still using the old Bad Request policy — 2026-11-15

Decision: adopt the already-installed managed recovery fix in the affected
session, preserve its conversation, and verify the actual package's recovery.
Do not modify history or tool schemas without evidence of an invalid field.

The affected session is `s-eb8457efc9e7f8275433`, conversation
`01a1ce34-6fad-7e12-9524-de5dae0abb9f`. Its screen reports codex-alab 0.7.0,
based on OpenAI Codex 0.168.0, with a stopped request/configuration attention.
The supervised process is PID 14016. Its process birth is November 12; process
birth alone does not date its last executable image. No old live-image hash was
collected. The screen and observed permanent-stop behavior identify the old
client policy; they do not prove which historical package's bytes were mapped.

This thread uses paginated SQLite history. Its legacy rollout is not the current
error authority. A read-only thread_turns query retains failed turn
`01a1da66-74ed-7371-96f0-1f02ca5b0c53`, with message
`{"detail":"Bad Request"}`, codexErrorInfo other and additionalDetails null.
The matching old-process log records the turn error at INFO. These observations
supply no rejected field, wire request or service request ID. The backend reason
remains unknown; none of the proxy, schema or history hypotheses is established
by this incident. The demonstrated client defect is permanently parking on the
opaque rejection while the fixed package is already installed.

Managed package `2026111502` reports ALab 0.9.0, upstream baseline 0.169.0 and exact
source `cdfe65abe31e6cf27ed2ea3e0028a07c283ad63a`. The package's executable hash is
`b10d77a154d132888e60be1f39de5af5559a4c146d21eb8318285ea052267b34`.
That source already contains opaque-envelope normalization, bounded request
retry, clean HTTPS fallback and paced Autopilot continuation that respects the provider
without the repeated-outage cutoff hold. Existing native regression records
remain the source evidence; this action builds or installs nothing.

Before restart, the existing session had an empty composer, no human hold, more
than 120 seconds since human input, and no running job. A private handoff was
saved, then aterm's validated restart route was used for that exact supervised PID. The
session resumes on the same conversation and project under PID 12670, reporting
codex-alab 0.9.0. An explicit continuation authorized by the person's current
request clears the legacy stopped state; it does not invent missing historical
Retry-After clocks. The old failed turn remains failed. No account change or manual history deletion is performed. The handoff directs
the agent to preserve completed work and inspect its recorded results before
continuing; the separate smoke below verifies single tool execution across retry.

Drive writes are protected by an owned lease, current supervisor and unchanged
human-input epoch. One stale control syntax initially writes literal `--stdin` into
the composer without submitting a turn. That exact text is cleared under guards;
other attempted writes are safely skipped by their fences. The actual inline
continuation and subsequent working turn are retained separately, correcting
the first attempt's intended-message metadata rather than silently relabeling it.

New turn `01a1e1f9-e14d-74f1-b495-4e36aec25d97` is in progress in the original
conversation. The live screen shows continuation of the original source work,
actual tool results and newly launched native subagents. New-process logs retain
model-visible items, Responses SSE activity, completed tools and the richer
alab_request_diagnostics target. These are observed resumed-work facts, not a
guarantee that a remote service will never reject another request.

The maintained request_recovery smoke was run against that existing managed package.
Each case has fresh homes, scrubbed environment, read-only work and an unauthenticated
loopback provider. It uses no real account. All four cases pass, and the outer
smoke process exits zero:

| Case | Requests | Raw command exit | Observed result |
| --- | ---: | ---: | --- |
| overload (503) | 3 | 0 | recovered after 2.006192 seconds |
| exact opaque 400 | 3 | 0 | recovered after 2.005385 seconds |
| specific invalid_prompt | 2 | 1 | explicit refusal retained; no retry |
| array-shaped 400 | 2 | 1 | non-opaque refusal retained; no retry |

For both successful recoveries, the strict oracle verifies the original input
prefix, exactly one completed update_plan call with its original arguments and
successful output in order, identical input on retry, one final answer and one
completed turn. Delay is measured from response headers and honors the two-second
Retry-After. The negative cases require one failed turn and no successful turn.
Exact guardian argv, raw status and non-timeout evidence agree. The executable's
before/after hash is unchanged. Independent read-only review rechecks requests,
events, command receipts, package source and binary identity.

The raw smoke result hash is
`d1b4ddf2dd4af90e7943867655eec3393c3254f1a742318a2e32904f38282a40`.
This closes the earlier positive installed-package smoke deferral. It does not
change any earlier failed build/test record or certify a newly built application.
Specific invalid requests remain distinct from opaque gateway errors; blindly
replaying an identified bad configuration is not this recovery contract.

Leave the pre-existing session running its original task. The source recovery
fix was already carried and published; the necessary repair here is adopting it
in the stale process and validating the delivered application, not adding an
unrelated source change or claiming an unproven backend root cause.
