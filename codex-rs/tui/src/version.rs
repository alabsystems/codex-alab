/// The version captured by the current Codex executable, with a Cargo source fallback.
pub fn cli_version() -> &'static str {
    // ALAB(version): running executable identity
    codex_alab_tui::session::version() // ALAB(version): the running executable owns this identity
}
