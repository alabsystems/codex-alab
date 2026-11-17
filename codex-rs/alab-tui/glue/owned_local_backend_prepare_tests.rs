// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source IO precondition only. This cannot construct or qualify a Native bind.
use super::*;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;

struct AlreadyWithdrawn;
impl IncomingStreamProvider for AlreadyWithdrawn {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::pending())
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(std::future::ready(()))
    }
}

#[tokio::test]
async fn withdrawn_source_input_denies_before_ordinary_preparation() -> color_eyre::Result<()> {
    use clap::Parser;
    let cli = Cli::try_parse_from(["codex"])?;
    let result = prepare(
        cli,
        Arg0DispatchPaths::default(),
        LoaderOverrides::without_managed_config_for_tests(),
        Incoming::new(AlreadyWithdrawn),
    )
    .await;
    let error = match result {
        Ok(_) => color_eyre::eyre::bail!("withdrawn input entered ordinary preparation"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    Ok(())
}
