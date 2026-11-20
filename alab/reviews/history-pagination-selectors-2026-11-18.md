# History pagination on the 0.170 integration

The protected TUI stage on candidate `fcb2205723` returned zero after three
upstream renderer tests. The campaign required eight distinct tests for its
four history-pagination scenarios and rejected the result. Its original log
has SHA-256 `3444867d725c6ce398d5415a27754e8b8fae28d52daedfc8aa63eb351d0269a9`.

The renderer delegates grouping to the ALab implementation in
`app_server_session::history_page`. Its five tests exercise reversed
completion offsets 76 and 38, interleaved ownership, reused item identities,
metadata permutations on partial pages and the page that owns an empty
interruption. The recipe still named them under the renderer's former test
module. Topic `80330e42c8` selects the five tests at their compiled names
and keeps the three upstream tests under the renderer. The same interleaved
case contributes evidence to both item ownership and interruption ownership.
[Execution recipe](../feature_recipes/scenarios.json) · [Grouping cases](../../codex-rs/alab-tui/glue/history_page_tests.rs) · [Renderer cases](../../codex-rs/tui/src/app/history_completion_tests.rs)

An independent protected run used source-selected Trust 15698 and the same
clean source `fcb22057237643ce44c38ddf6a9078845734562f`. All eight exact
test names passed once. The log SHA-256 is
`cb028da88f878b3b5815578064eeda44c701a69c5b821e27045020e052170700`.
The native evidence reader accepted each of the four scenario selections
against these bytes. It rejected the retired names, a deliberately unexecuted
name, and the corrected eight-test declaration against the earlier three-test
log. Thirteen focused-result tests, 29 evidence tests and 21 campaign tests
also passed.

The original partial attempt stays failed. These standalone observations
justify the corrected declaration. The campaign can collect it under the
corrected recipe digest when its active worktree becomes available.
