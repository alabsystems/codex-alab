// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native lab entry point. The host must independently admit this actual test
//! image and issue its constructor; this executable supplies no native grants.

#[cfg(not(debug_assertions))]
compile_error!("native SDK probes require debug assertions");

#[path = "../../alab-core/glue/native_source_proof_backend.rs"]
mod backend;
#[path = "../../alab-core/glue/native_source_proof_cli.rs"]
mod proof;
#[path = "../../alab-core/glue/native_source_proof_retained.rs"]
mod retained;

#[path = "../../alab-core/glue/native_source_proof_bound.rs"]
mod bound;
#[path = "../../alab-core/glue/native_source_proof_capabilities.rs"]
mod capabilities;

fn main() -> std::io::Result<()> {
    if !proof::requested()
        && !backend::requested()
        && !retained::requested()
        && !bound::requested()
        && !capabilities::requested()
    {
        return Err(std::io::Error::other(
            "native source proof command required",
        ));
    }
    codex_alab_core::activate();
    codex_alab_core::initialize_running_build!().map_err(std::io::Error::other)?;
    // Install the same transport and credential adapters as the production CLI.
    codex_alab_tui::life::capture_launch();
    if capabilities::requested() {
        return capabilities::run();
    }
    // Use the ordinary CLI/app-server runtime and its shared stack policy.
    // The arg0 guard remains alive until this exact private dispatch finishes.
    codex_arg0::arg0_dispatch_or_else(|arg0_paths| async move {
        let result = if bound::requested() {
            bound::run(arg0_paths).await
        } else if retained::requested() {
            retained::run(arg0_paths).await
        } else if backend::requested() {
            backend::run(arg0_paths).await
        } else {
            codex_alab_core::native_source_launch::enter().await?;
            proof::run().await
        };
        result.map_err(Into::into)
    })
    .map_err(|error| match error.downcast::<std::io::Error>() {
        Ok(error) => error,
        Err(_) => std::io::Error::other("native source proof runtime unavailable"),
    })
}
