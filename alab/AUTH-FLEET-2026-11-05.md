<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Native authentication and shared-account fleet check, 2026-11-05

Account enrollment, email access, account lists and machine identity use aterm's
general services. Codex consumes native account selection and admission; it does
not implement a Proton mailbox client. This record separates live provider
observations from isolated account-rotation tests.

## Native recovery qualification

Codex source `8b00fe6315a8259b3a141156e00343d8895d502a`, packaged privately as
`0.1.0-dev.14789+codex.0.166.0.g8b00fe631`, passes all 17 native scenarios against
aterm host `4e90b1b7ce5b0dd5dc8a0024f4b703ff1cd8c250`. These include persistent
and ephemeral review recovery, ephemeral roots and active/idle children,
repeated switches, accepted mail, token usage, lost consume acknowledgment,
human output, changed configuration/worktree, exhausted pools and owner Stop.
Forty simultaneous isolated logins and eight sessions sharing repeated quota
rotations also pass. These are real processes using isolated fixture credentials
and a local provider, not live provider enrollment.

The package passed its canonical build and packaging checks with `--no-promote`.
Its `bin/codex` SHA-256 is
`e0dec05e23c8d80ecb12e355f6cfcbbe9edb3d62031292c88cd6768af0b4fb98`.
Native results are retained in
`$HOME/.aterm/.ubd/codex-live-native-8b00fe631-v1/summary.json`.
The full `alab/check.sh` gate also passed on that source: the CLI build/run,
ALab tests, owned-code lint and hooked upstream unit suites. Core passed 3,059
tests with one ignored, Exec passed 71, and TUI passed 6,186 with five ignored.
The retained gate log is `codex-live-20261105-full-gate.log` under the same
private evidence directory. Later incoming interface changes are not part of
this tested snapshot.

## Live observations

The native Codex app-server `account/read` and `account/rateLimits/read` methods
confirmed three distinct existing provider accounts on m158, m22 and m8. Weekly
usage was respectively 78%, 100% and 83%. The exhausted account reported ordinary
usage unavailable; the other two reported it available. These are point-in-time
provider reads, not model requests or proof of automatic cross-machine rotation.
The m8 cached roster had reported 46%, so cached usage is not live quota evidence.

On m8, installed aterm 0.154.0 could not finish reading the shared catalog within
its application deadline. A private debug build of aterm source
`7bec0e5ea3662b43d610ecff1b3723749e65a3a3` completed the same native account-list
read in 2.881 seconds. It returned 19 retained records, complete history and no
source errors. Its proposal table, import table and all 19 account records matched
the native m158 read. This verifies shared catalog agreement on two machines;
candidate records are not evidence of credentials or enrollment authority.

On m22, catalog access remained refused because the table's filesystem device
number had changed without a persistent volume witness. Native custody inspection
reported a verified receipt prefix but no authenticated application or permission
to resume the writer. No claim, receipt or volume identity was rewritten.

## Enrollment limit

Aterm's native renewal for the pending account stopped at Cloudflare on
`auth.openai.com/oauth/authorize`. Its native interactive browser flow then opened
the account's existing profile, but timed out without a confirmed sign-in. The
subsequent renewal waited for the existing machine sign-in lock to be released
naturally and made one attempt using that profile. Cloudflare did not clear in
90 seconds; aterm reported that no live authentication state changed. No lock was
broken, browser challenge bypassed or enrollment success inferred.

The additional live login and automatic rotation between live provider accounts
therefore remain unverified. The private m8 build was not installed or promoted;
all owned browser and remote shell sessions were closed after measurement.

## Human challenge handoff follow-up

Aterm `309f0d46ab` adds quiet, account-scoped human login tasks for confirmed
browser challenges. Its existing account readers expose the native login step;
configured vendor pools also retain one informational Messages entry. It adds
no Codex email client or repeated prompt into working agent sessions.

A private debug build from `f314cf1123` made one further native renewal attempt.
Cloudflare again stopped authorization, but this time aterm recorded the human
task. The installed aterm's account JSON reader confirmed `codex-m22` still not
enrolled, with `sign_in_task.action = login` and the next step
`aterm harness accounts login codex-m22 --cli codex`. No interactive browser
window opened and no binary was installed. The live login remains unfinished;
the handoff to a human is verified.

Aterm's agent verification passed formatting, census, lint, doctests, cross-platform
checks and its test suites. The GUI suite also passed, including its quiet-message
regression. See aterm's `docs/measured/human-auth-challenges-2026-11-05.md` for
the exact tested revisions and private evidence filenames.

## Retained evidence

Private results are under `$HOME/.aterm/.ubd/`:

- `live-provider-{m158,m22,m8}-20261105.json`: sanitized provider observations.
- `live-m8-private-build-20261105.json`: private build and catalog result.
- `live-shared-catalog-comparison-20261105.json`: matching catalog records.
- `live-m22-custody-20261105.json`: native refusal and custody metadata.
- `codex-live-enrollment-after-lock-20261105.log`: final native renewal result.

These observations supplement the isolated native recovery qualification; they
do not turn fixture credentials into live enrollment proof or complete the
parallel-login goal.
