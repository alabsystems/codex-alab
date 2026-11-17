// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real existing capability reporter. No canned fields or Native permission.
use clap::Parser as _;
#[derive(clap::Parser)]
struct Args {
    #[command(flatten)]
    command: codex_alab_tui::upgrade_target::Command,
}
pub(crate) fn requested() -> bool {
    let args = std::env::args_os().collect::<Vec<_>>();
    args.get(1).is_some_and(|arg| arg == "debug")
        && args.get(2).is_some_and(|arg| arg == "upgrade-capabilities")
}
pub(crate) fn run() -> std::io::Result<()> {
    let args = std::env::args_os()
        .enumerate()
        .filter_map(|(index, arg)| (index == 0 || index >= 3).then_some(arg));
    let parsed = Args::try_parse_from(args)
        .map_err(|_| std::io::Error::other("native proof capability arguments unavailable"))?;
    parsed.command.run()
}
