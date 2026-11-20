<!-- Copyright 2026 Andrew Yates. SPDX-License-Identifier: Apache-2.0 -->

# WebRTC credentials during call creation, cancellation and local resume

Both the reviewed ALab CLI at `21e660cf42` and official OpenAI Codex 0.170.0
resumed the original thread under the replacement login and started voice with
that login's bearer and workspace on both network requests.

| Action in the same original thread | Local call creation | App-server observer | Terminal outcome |
| --- | --- | --- | --- |
| Replace A’s on-disk login with B after the provider receives the first offer and before returning its answer | A | A | Voice starts and stops on request |
| Start a second call and cancel while its provider answer is held | B | No request for the cancelled call ID | A new call can start immediately |
| Start that new call in the original process | B | A | The owner-checking loopback service rejects the observer with HTTP 403 and the terminal reports it |
| Exit, then run `codex resume <original-thread-id>` under B | B | B | Voice starts and stops on request |

The TUI reads local provider credentials when it creates each call. The
embedded app-server resolves observer credentials from its running auth
manager. External writes to the credential file take effect in that manager
after an explicit reload or a restart. A local credential-file replacement
can therefore put the two voice requests under different accounts until the
app-server restarts. The service in this test requires matching owners. Both
builds surfaced its refusal and recovered through ordinary local resume.
See the [TUI call creator](https://github.com/openai/codex/blob/rust-v0.170.0/codex-rs/tui/src/realtime_call.rs),
[observer attachment](https://github.com/openai/codex/blob/rust-v0.170.0/codex-rs/core/src/realtime_conversation/existing_call.rs)
and [AuthManager snapshot contract](https://github.com/openai/codex/blob/rust-v0.170.0/codex-rs/login/src/auth/manager.rs).

## Reproduce

On macOS or Linux, use Python with `os.waitid` and `WNOWAIT`, a Codex CLI,
and a fresh output directory.

```sh
python3.14 -B alab/smoke/voice_account_resume.py --codex /path/to/bin/codex --output /tmp/voice-account-resume
```

The smoke drives the real `/voice` command in a PTY, gates HTTPS call creation
on a loopback service, and observes the app-server WebSocket handshakes and
disconnects. Each successful observer closes without sending application data.
The private test package supplies a synthetic media helper and SDP. Device
capture, playback, ICE negotiation, and the hosted service's authorization
policy are outside this measurement. The output's `result.json` reports
synthetic account labels, call and thread identifiers, and executable hashes.

| Executable | SHA-256 |
| --- | --- |
| ALab candidate `21e660cf42` | `df83b4ce3aae0437b5d3435f8c90140cc316e8309ce7f274bf9f189d1d33f55a` |
| Official OpenAI Codex 0.170.0 | `9248cfeebec9b4cc3034d5114caab10fbf426cbfa79dba410c5fa1b72762abd6` |
