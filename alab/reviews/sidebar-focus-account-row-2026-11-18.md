# OpenAI Codex 0.170 sidebar focus and the ALab account row

The upstream focus change at `ae50f973f5` should leave the displayed side chat in
place while focus moves between composers. The ALab footer follows the active
composer. The terminal control in `alab/smoke/sidebar_account_focus.py` drives
the actual binary against a local HTTPS model and usage fixture. Each run gets
its own empty home, synthetic ChatGPT login, and real PTY.

## Reproduce

With a Python that supports `os.waitid` and `os.WNOWAIT`, pass a fresh directory
for each run. The two-CLI control seeds an attached side chat in the same home,
closes the seed process and opens its original parent with `codex resume UUID`.

```sh
python3.14 -B alab/smoke/sidebar_account_focus.py --codex "$candidate" --output "$results/cold"
python3.14 -B alab/smoke/sidebar_account_focus.py --codex "$candidate" --seed-binary "$signed" --output "$results/saved"
python3.14 -B alab/smoke/sidebar_account_focus.py --codex "$vanilla" --cold-start-only --output "$results/vanilla"
```

The full control submits one parent turn and opens one child turn with
`/side`. The parent and child have unique assistant replies and different
unsent drafts. The test sends the actual Ctrl-X s and Ctrl-X c bindings,
appends a different suffix on each side and submits the parent draft. It
asserts the original parent ID, a separate child ID, exactly one opening
prompt, and the synthetic account and workspace on all three model requests.

Each focus check triggers a terminal resize and reconstructs the newly painted
cursor-addressed rows. It requires both replies and both drafts in their
respective panes. The active pane must display the complete synthetic login
and the fixture's weekly usage on the same row. The allowed display forms
show 23% used or 77% left. A negative control using Ctrl-X c in place of
Ctrl-X s fails the side-focused frame.

## Observations on macOS arm64

| CLI under test | Cold side creation | Focus on an already attached side chat |
| --- | --- | --- |
| OpenAI Codex 0.170.0, SHA-256 `9248cfeebec9b4cc3034d5114caab10fbf426cbfa79dba410c5fa1b72762abd6` | Both replies appeared in their own panes. The second request used the child ID and the same synthetic account. | The account-row check is ALab-specific. |
| Signed ALab 0.11.0, package 2026111701, SHA-256 `090aacf5157ce4326da1744ab8869218dff31dc4fb7bde1f9bd226736f7022b3` | Cold creation and the full focus/row control passed. | Seed for the saved-chat comparison. |
| Source-selected development helper from frozen `21e660cf42`, SHA-256 `df83b4ce3aae0437b5d3435f8c90140cc316e8309ce7f274bf9f189d1d33f55a` | After the loading indicator, the process aborted before sending the child prompt. The PTY custodian observed exit -6 and reaped its owned group. | The full focus/draft/footer control passed on the original parent, including the final parent submission. |
| Corrected 0.170 integration development helper from `5ca190d357`, SHA-256 `762c5f3442bf6904872ced0879224f3da23ab1bb60a918c671a030900d4c3150` | The full account, two-draft and panel control passed on the default 16 MiB stack. All three turns reached the expected parent, child and parent IDs. | The signed-seed resume control also passed. Both runs exited with status zero and their PTY groups were reaped. |

The original macOS crash report identifies the Rust stack-overflow signal
handler on `codex-main` during `App::handle_start_side`. The call site in the
arm64 development helper transfers a 344,128-byte task into Tokio's generic
`spawn`. Its generated frame reserves another 688,384 bytes plus saved
registers. The launcher explicitly gives `codex-main` a 16 MiB stack.
Repeating cold creation with `RUST_MIN_STACK=67108864` produced the same abort.

The source change at `5ca190d357` uses the TUI's existing non-inlined
`boxed_future!` at the sidebar fork factory. It moves that original future
to the heap before the UI queues it while preserving its request view,
backend identity, capabilities and side-creation completion.

On the corrected arm64 helper, the task passed to Tokio occupies 192
bytes and its generated spawn frame reserves 512 bytes plus saved registers.
Both task sizes come from the respective instantiated
`tokio::task::spawn::spawn` functions. Both helpers used the repository's
unverified CLI test prerequisite, source-selected Trust 15698 and the
unoptimized Cargo development profile with debuginfo disabled. The frozen
test package is identified by its source and executable hash.

Only the existing `codex-rs/tui/src/app_server_session.rs` factory changed.
At the measured upstream checkout `alab/delta.sh` reported churn 124,
risk 29,388 and +173/-64 lines before this change. Afterward it reported
churn 124, risk 29,884 and +175/-66. The two-line hunk reuses the existing
`ALAB(startup-stack)` topic. Feature inventory reported ownership complete
for 113 topics across 14 features on the 0.170 source and on its main-line
port `641b7d6127`. The 22 existing PTY custodian tests also passed.

The main-line port was independently built with source-selected Trust 15698
against upstream frontier `ce3031018e`. Its test executable has SHA-256
`2e3c7daae95d9234bd24dda29c4e83f44e4f018f0bff29aed08656b4d8d2ea64`.
The full cold PTY control passed with its normal stack setting. It verified the
initial, parent, side and returned frames, observed the three expected requests
on the parent, child and original parent, and reaped the process cleanly.

## Fresh and resumed side chats on the 425-commit integration

The macOS ARM64 CLI at `3431c96dfd` opened the parent, then aborted on its
normal stack while selecting the attached side chat. A conversation seeded
by signed ALab 0.11.0 also aborted when the candidate selected its child.
macOS recorded a stack overflow in the TUI's thread-snapshot rendering path.
On the 0.12 CLI at `61bc78c131`, both fresh and resumed terminals completed
the same focus, draft and request checks.

The repair puts the shared thread-selection future on the heap using the
existing TUI allocation helper. At `c874746c62`, the fresh terminal and
the terminal resumed from signed 0.11.0 both showed separate parent and
child drafts. In each trial the three requests reached the parent, child
and original parent in order. Both candidate processes exited normally
at the default stack size. The same executable passed all 19 direct model
controls, including recovery with a running shell. Its SHA-256 was
`36cdbcd27f2ac5139c730beb46e4f6d3da88cf55a636dc6fc0814238a25e8014`.
[Source of the tested candidate](https://github.com/alabsystems/codex-alab/tree/alab/alignment-0.170-c874746c62)

The native UI campaign now runs fresh and resumed terminal controls for
conversation switching. The candidate creates its own saved conversation
through the public TUI before the resume check. The independent signed-seed
trial above covers compatibility with the installed release.
