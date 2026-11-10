#![allow(clippy::expect_used)]

// Single integration test binary that aggregates all test modules.
// The submodules live in `tests/all/`.
pub use codex_protocol::error;

mod suite;

#[path = "../../alab-core/glue/ordinary_suspension_tests.rs"]
mod alab_ordinary_suspension; // ALAB(accounts): real ordinary suspension must not migrate unrelated source owners.
