<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Headless authentication validation

## Live audit, 2026-11-02

The requested end state remains unverified: unattended OpenAI login, retrieval
of the resulting email code, quota recovery, and unattended handling of missing
credentials must all work together. Existing quota unit tests are not evidence
that the complete authentication path works.

On this Mac, `aterm ctl @self status` answered successfully. The secrets registry
listed `ai-accounts.csv` and `ai-mailbox.toml`, and vault membership was active.
Those metadata establish discovery, not working account or mailbox credentials.
No secret values were printed or copied.

`aterm harness accounts list --cli codex` reported the local renewable row
`codex-m22` as not enrolled. There were initially no Codex account ledger events.
The authorized live attempt was:

```sh
aterm harness accounts renew codex-m22 --cli codex --here
```

It exited 1: no headless browser was installed. The command reported that only
the attempt was recorded and no live login changed. This is an installation
dependency failure; it does not establish that mailbox credentials are missing.
Do not request replacement credentials on this evidence.

The aterm source's `VendorApprover::approve` in
`crates/aterm-agent/src/harness/headless_oauth.rs` discovers a browser and stops
when none exists. Its `headless_fetch.rs` already contains a pinned, size- and
SHA-256-checked browser installer, but documents its automatic headless fetch
route as Grok-specific. Next: make that existing bounded installer available to
Codex renewal, verify failure and cancellation behavior, then retry this exact
row through staging. Respect any provider challenge or explicit hold.

Completion requires evidence of code delivery and retrieval for the intended
account, staged identity verification and publication, and recovery of the same
task after a quota reset. A browser installation or successful login alone does
not prove those requirements. Missing credentials should use the existing
secrets-need registry with the exact failed route and required credential type;
neither routine dialogs nor repeated requests to the person are recovery.

## Browser dependency removed, 2026-11-02

The aterm checkout's supported command
`targo --unverified verify provision chrome-headless-shell` passed and installed
its pinned 155.0.8059.84 mac-arm64 browser. Retrying the same account renewal
launched the headless browser and reached OpenAI. Cloudflare's bot check on
`auth.openai.com/oauth/authorize` did not clear within the existing 90-second
limit. The renewal exited 1 with `browser bot-check` in the account ledger;
no live login changed. Do not bypass this provider challenge, infer a password
failure, or repeatedly retry the unchanged route.

An isolated aterm worktree at
`$HOME/.aterm/worktrees/codex-headless-auth` now connects vendor renewal
to `ensure_headless_browser`, the existing pinned installer, with cancellation
when the CLI finishes or its sign-in deadline expires. Validation is pending;
this source change is not yet landed or installed. Its regression test exercises
both lifetime limits without launching a browser or downloading anything.

## Installed package capacity check

`alab/smoke/capacity_exec.py` passed against installed package `2026102801`,
source `1d44d61087384be32031bf636534db037397431a`. Its local-server cases
proved task preservation and recovery after a transient capacity refusal (three
requests, 2.004 seconds after response headers for a two-second Retry-After),
and termination after a permanent refusal (one request). Retained artifacts are
in `$HOME/.aterm/.ubd/codex-goal-capacity-20261102.noindex`.
The smoke uses isolated homes and no real account. It does not exercise account
quota reset, live allowance restoration, or email-code authentication.

## Headless account-quota gap, 2026-11-02

A local fixture returning 429 `usage_limit_reached` with a future `resets_at`
timestamp reproduced a terminal `turn.failed` in installed package `2026102801`
after the plan tool completed. Unlike transient capacity recovery, the installed
headless executable did not wait for account quota reset. Artifacts from the
retained `--quota` probe are in
`$HOME/.aterm/.ubd/codex-quota-baseline-probe-20261102.noindex`.

Branch `alab/headless-quota`, in
`$HOME/.aterm/worktrees/codex-quota-exec`, adds exec-only quota waiting
through ALab's existing Responses retry glue. It waits for a provider reset,
or polls with at least 30 seconds of backoff when no reset is supplied. It
keeps the native turn and completed work, obeys Retry-After, and retains positive
backoff after stale resets. Interactive sources, credit depletion
and workspace spending caps keep their terminal behavior. Compilation and the
new quota probe against a built binary remain pending; this is not yet a shipped
or verified fix. The existing probe's 26 Python tests passed.

The initial Trust test was diagnosed by `aterm ctl @self sys jobs` as stuck on a
Chromium dependency TCP fetch. Only that owned process group was terminated.
The replacement test uses Cargo's command-line Git fetcher with a bounded
low-throughput timeout and has advanced through further pinned submodules.

The aterm installer's 13 focused tests and its new login-lifetime test passed;
formatting and source censuses passed. The wider verifier is still running.
Its missing rustup tool was provisioned successfully, and the previously skipped
four cross-platform cells were started separately. No proof lane was run.

The supported device flow was also attempted with the configured account identity
in a private scratch roster and login directory. It reached the same Cloudflare
check and stopped after 90 seconds; no live login changed. Do not repeat either
unchanged route or treat this as evidence that mailbox credentials are absent.
The four aterm cross-platform cells subsequently passed, as did its full agent
test suite. Its native lint stage is still running.

## Runtime evidence and remaining scope

The executable built from `0793df0a55` passed six local end-to-end cases in
`$HOME/.aterm/.ubd/codex-quota-routed-20261102.noindex`: transient
capacity, a future account reset, and an omitted reset all recovered in the
same turn with completed work preserved; their permanent-refusal controls each
stopped after one request. The omitted-reset case waited 30.013 seconds. These
results prove fresh exec recovery, not resumed-thread recovery.

Review then found that a resumed thread retains its original CLI/VSCode source.
`d49f366277` replaces that source check with volatile, per-registry Exec-host
admission, without rewriting history or admitting internal/child threads. Its
lifecycle tests and the new `quota_resume.py` native protocol probe remain to be
run against the final executable. The previous executable is retained, with its
checksum verified, as a negative control.

aterm browser provisioning is on remote main. The subsequent change classifying
browser preparation failures as `NotRun`, without a person's sign-in task, is
also on remote main at `c485e245db`. A follow-up audit found that the worktree's
submodule checkout still held the earlier astream revision after main advanced;
validation is being repeated with the committed `222db4e6cf` pin. Do not treat
the earlier verification as proof of that composed dependency tree.

The native account report currently returns usable usage for the default Codex
login, but its roster row has no email and is not renewable. Automatic metadata
completion is being implemented through the account-management path. The
registered mailbox config has an explicit default login and password; that
default is a supported source even without a per-address entry. Its access and
delivery of mail for the intended Codex account are separate unverified claims.
The connection-only probe must honor the shared login cooldown and must not
consume codes or message digests.

## Final-source local checks, 2026-11-02

The `6cec9bfd10` tree built both `codex` and `codex-app-server` successfully.
Its quota lifecycle tests passed (4), as did the Responses retry tests (21;
1 existing ignored). The final built executable passed all ten fresh-session
local cases: capacity recovery, known and unknown quota reset recovery, their
permanent-refusal controls, and four HTTP 429 credit/spending-cap controls.
Evidence: `$HOME/.aterm/.ubd/codex-quota-final-fresh-20261102.noindex`.
These are development binaries, not validated production packages.

The three native resumed-history cases also passed: CLI history resumed by exec,
VSCode history resumed by exec with an omitted reset, and Exec history resumed
by a non-exec host. Saved provenance remained unchanged. The preserved old binary
failed the first case with two requests and `turn.failed`, establishing that the
probe detects the prior defect. Evidence lives in
`$HOME/.aterm/.ubd/codex-quota-resume-final-20261102.noindex` and
`$HOME/.aterm/.ubd/codex-quota-resume-negative-20261102.noindex`.
Final-source lint remains pending.

With the committed astream pin, aterm's formatting, source censuses, cross-platform
cells, native compilation, lint and doctests passed. The first complete test run
had two disk-cleanup failures; all three related cases passed in isolation, then
the complete native unit and integration test stage passed on rerun. Logs:
`/tmp/codex-auth-no-nags-pinned-verify.log`,
`/tmp/codex-auth-disk-rerun.log`, and
`/tmp/codex-auth-no-nags-pinned-tests-rerun.log`.

The default mailbox connection and mailbox-key unlock are now verified. The
single connection attempt waited until the actual native renewal holder released
its machine-wide sign-in lock, then checked holds and cooldowns before connecting.
It read no messages or codes and changed no credential source or hold. Evidence:
`$HOME/.aterm/.ubd/mailbox-default-probe-20261102.noindex/run-after-holder/result.json`.
This proves configured mailbox access, not fresh password authentication,
forwarding to the intended Codex identity, or completion of OpenAI email-code
login. Those, automatic account-metadata completion, and installation of the
validated changes remain outstanding.

The composed `98e9948821` source (including explicit fallible quota-wait
admission and current bootstrap fixes) passed its four host-lifecycle and 21
retry unit tests, with one existing ignored test. Its rebuilt `codex` and
`codex-app-server` also passed all 15 local runtime cases: ten fresh-session
cases, three persisted-origin cases, and the two repeated-wait/cancellation
cases. The latter interrupted a distant quota reset through the native SIGINT
handler, without a further HTTP request or guardian timeout. Artifacts are
`codex-quota-composed-{fresh,resume,wait}-20261102.noindex` under the private
`.aterm/.ubd` directory. Final composed-source lint is still pending.

A wider recovery audit found an additional interactive login gap: aterm can
publish the same account's renewal into a new folder while running Codex still
watches, and restarts with, the original auth path. The existing same-file login
watcher cannot observe that move. Work on a same-account native renewal handoff
is isolated from the quota branch; it must validate account/user identity and
current authority, use the existing private restart machinery, and durably
recover the original failed turn without duplicating accepted work. This gap,
real email-code sign-in, and deployment remain unverified.

The final quota lint command exited zero. Its 18 warnings point only to upstream
lines outside ALAB hooks; none point into ALAB crates/glue or hook lines. The full
app-server library run passed 447 tests and could not run one MCP-startup case
because the standalone `test_stdio_server` fixture had not been built. The full
core library run and builds of the separate MCP/code-mode fixture executables
are in progress. These missing executables are test prerequisites, not evidence
that the full library suites passed.

The full affected-library validation is now complete: 448 app-server tests and
2,932 core tests passed, with one existing core test ignored. The six cases
initially missing executable fixtures passed when supplied with the actual
built `test_stdio_server` and `codex-code-mode-host`. The latter used the
repository's verified, pinned Codex V8 pair after the default upstream artifact
URL returned 404. The current-main CLI rebuild is still running.

Using the locally built aterm enrollment change, completion of the existing
`main` Codex row succeeded after the machine sign-in lock released (five bounded
lock refusals before success). Both that binary and installed aterm now report
email present, `renews=true`, and `condition=usable`. This operation changed only
account metadata; the new enrollment implementation is not yet installed.
Sanitized evidence is `codex-default-completion-retry-20261102.json` in the
private `.aterm/.ubd` directory. It does not prove a new OpenAI sign-in.

After composing current main's independent TUI hardening at `2e92475ca3`, the
CLI rebuilt successfully and all 15 local runtime cases passed again. The
core/app-server source did not change in that composition, so the completed
3,380-pass/one-ignored affected-library results and clean ALAB lint findings
remain applicable. The quota implementation is ready to land independently of
the still-unqualified account-renewal handoff.

The canonical package build of `e1a4c8c999269ec004f42c18b98ff46420e4bbad`
completed with version `0.1.0-dev.14439+codex.0.166.0.ge1a4c8c99` for
`aarch64-apple-darwin`. All 15 runtime cases passed against that packaged
`bin/codex`: capacity (2), known quota reset (2), omitted reset (2), terminal
credit/spending refusals (4), repeated wait and cancellation (2), and persisted
thread resume (3). The resume fixture used the separately built app-server to
seed and inspect history; the resumed Exec process used the packaged binary.
These runs used scrubbed environments, fresh homes, and loopback fixtures; they
do not establish live account authentication or OS-level account containment.

Retained results are the six `codex-quota-packaged-{capacity,quota,unknown,
terminals,wait,resume}-20261102.noindex/result.json` files under the private
`.aterm/.ubd` directory. The first four record the package's canonical build and
manifest metadata. The selected managed package changed to `2026110301` during
the build; its source is `ef71b0aa49fca48946749b43245f6f7eb17f51de`. That
selection was preserved. The tested candidate remains unlinked, so these
results do not assert that the running or selected Codex has adopted it.
