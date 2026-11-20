# Execution continuity after a competing shell snapshot is canceled

The seventh execution-continuity scenario requires the first snapshot to stay
usable while a second generation is canceled during validation. The existing
test waited for real validation to start, canceled that task, examined the
first filename, and dropped its owner. A truncated or unusable first file
could satisfy those filesystem checks.

The fixture now captures different exported values for the two generations.
After canceling the second validation, it removes the capture inputs and runs
a command through the production shell replay wrapper with the original
snapshot. The command must emit the first generation's value. After dropping
the owner, the same command and path must emit the fresh shell's value.
[Snapshot regression](../../codex-rs/alab-core/glue/shell_snapshot_tests.rs) · [Execution contract](../features.toml)

## Remaining producer work

The project already carries native tests for the other scenarios. The first
four concern hook defaults and explicit settings, model migration, MCP
elicitation and title generation. The ALab policy tests, startup and rename
TUI checks, and app-server MCP checks are candidate selections for these
cases. Each selection must exercise the settings and command paths named in
its scenario.

Shell-output and pending-fork evidence requires two distinct Cargo integration
targets. The core package's `alab_shell_output` exercises clipped output at
the text and Code Mode surfaces. The CLI package's `alab_tool_result_recovery`
starts real product processes, holds a pending parent while its sample and
fork run, and reopens durable diagnostics after process replacement. It
also exercises rejected and interrupted responses, invocation binding, and
late stderr. The current campaign's `core-integration` selects a different
Cargo target. Collect the exact CLI and core target names against the frozen
source before admitting those scenarios. The independent snapshot check
covers one of the seven.
[Core integration target](../../codex-rs/alab-core/glue/shell_output_integration.rs) · [CLI integration target](../../codex-rs/alab-core/glue/tool_result_recovery_integration.rs) · [Campaign runner](../feature_recipes/run.sh)

The source-selected Trust 15698 core stage executed
`shell_snapshot::alab_snapshot_tests::try_create_cancellation_removes_only_the_pending_generation`
on `5f481a46172f8e627ddc5b225edcc72ac06cc343`. The test passed and the
focused-results reader rejected an unexecuted control selector against
the same log. Its SHA-256 is `f972ed9db56aaa4639c5adc9cb74448e2f0e6e14fc863b52cc6947adeac096cb`.
