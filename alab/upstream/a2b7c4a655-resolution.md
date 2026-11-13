# Upstream a2b7c4a655 composition

The seven-commit intake records static decisions; runtime qualification is pending.

- `codex-mcp/src/runtime.rs`: retain the ALab assignment module and EnterpriseState export, followed by the new upstream Apps refresh test module. They serve independent ownership and test purposes.
- `tui/src/chatwidget/tests/status_and_layout.rs`: take the official equivalent consent fixture. Retire the two redundant ALAB(interface) markers; no production hook is removed.
- Automatic session, startup, TUI and config merges retain their existing marked ALab hooks. No ALab-owned AgentServiceTier implementation or removed Guardian config-field consumer was found.

The prior e13e full gate failed to attribute upstream unit failures because pristine 627b did not compile its consent fixture. This merge includes the official correction; it must pass a fresh gate, including pristine attribution. The e13e package and Aterm measurement do not qualify this composition.
