# Local providers and catalog Apps wording

Extend the reviewed upstream prefix from 25 to 28 commits, through
`83a23c09366d82cd0cb469c64c3e55d04d71c87a`, against ALab base
`d14bc8e6727837202cf111171f86e9bb86293dc2`. Preserve all prior decisions
verbatim and point each historical batch directly to the new active record.
The remaining 68 commits in the observed `f941a5fb92` snapshot are unreviewed.
This is review metadata, not a source merge or a passing application gate.

## `8cdabf23ae`: keep status-line providers opt-in

The new setting accepts the built-in default or a complete server/tool pair.
The merge replaces that identity as a unit; a partial higher-precedence
selection cannot inherit the other half from another layer. Project settings
and packaged defaults cannot select a provider. Effective-value and origin
reporting agree on that exclusion, while strict loading still diagnoses
malformed source values. Filtering the path-free setting before typed path
resolution prevents an ignored malformed provider from breaking unrelated
relative paths.

Remote-session configuration rebuilding now uses the client's local home for
local preferences, rather than the remote execution home. The selection moves
through `LocalSettings` and reloads into the chat widget. These commits only
establish configuration and persistence; they do not implement provider
discovery or periodic tool execution. No ALab default should opt in a provider.

Required checks include complete identity/profile precedence, malformed
project versus user settings, strict diagnostics, relative paths, and
`remote_resume_keeps_server_only_cwd_out_of_local_config`. Source search found
no direct ALab caller of the changed rebuilding methods or explicit literal
needing the added Config field; the integrated gate must still compile all
consumers and preserve ALab's local/remote ownership boundaries.

## `6c7ecc322f`: adopt persistence after a successful write

The new saver targets the active local user configuration path, including a
selected profile. App and widget state change only after the write succeeds.
It leaves the remote configuration and unrelated local settings alone. The
generic `SetPath` edit now returns an error when insertion cannot occur;
the insertion helper returns true for a successful assignment even if its
value was already equal, so an unchanged valid selection is not a write error.

The tests `remote_session_saves_to_the_active_local_profile` and
`failed_write_preserves_the_file_and_the_live_selection` cover both provider
forms, CLI override reload, malformed TOML, and a wrong-shaped parent table.
Require them on the integrated candidate along with ALab configuration and
profile round trips. The current saver is not yet wired to a provider picker.

## `83a23c0936`: keep catalog Apps instructions disabled by default

`catalog_apps_instructions` is under development and defaults off. Enabling
it changes wording only: Apps availability, policy and selected-model gates
still decide whether guidance is present. It does not authorize more tools.
The resolver preserves accepted bytes literally, caps nonempty text at 800
UTF-8 bytes, uses empty text to clear guidance, and falls back to bundled
wording for missing, whitespace-only or oversized values.

History reconciliation reads the latest matching developer fragment, issues
replacement/removal notices, and distinguishes literal notice-like catalog
text from generated notices. Both fragment markers remain contextual
developer messages in event mapping and rollback. The existing extension
bridge keeps its interface; the new internal reverse-iterator requirement
has no direct ALab caller found in source search. ALab's private history and
world-state codecs must preserve these fragments on the integrated candidate.

Required checks include literal/empty/byte-boundary resolver tests, model and
Apps gates, clearing and fallback, latest retained fragment after snapshot
loss, legacy checkpoint and downgrade behavior, and
`catalog_apps_instructions_follow_selected_model`. The provided source tests
cover those cases; they were read, not executed here. Keep the feature opt-in
until those native checks and the certified application gate pass.

## Validation

Validate exact 28-commit coverage, unchanged prior 25 decisions and one active
record per commit. No runtime or qualification result is inferred from a
valid intake record.
