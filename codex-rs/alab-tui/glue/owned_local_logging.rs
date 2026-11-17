// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original per-process logging setup shared by foreground and backend.
//! These resources remain in their creating process and confer no SDK rights.
use super::*;

#[path = "../../alab-core/glue/state_log_filter.rs"]
mod alab_live_logs; // ALAB(quiet-logs): same original dynamic filter in its shared logging owner.

pub(super) struct FileLogging {
    writer: tracing_appender::non_blocking::NonBlocking,
    filter: EnvFilter,
    guard: tracing_appender::non_blocking::WorkerGuard,
}

pub(super) fn prepare_file(config: &Config) -> std::io::Result<Option<FileLogging>> {
    let configured = config
        .config_layer_stack
        .effective_config()
        .as_table()
        .is_some_and(|table| table.contains_key("log_dir"))
        || config
            .config_layer_stack
            .requirements_toml()
            .log_dir
            .is_some();
    if !configured {
        return Ok(None);
    }
    std::fs::create_dir_all(&config.log_dir)?;
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(config.log_dir.join(TUI_LOG_FILE_NAME))?;
    let (writer, guard) = non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "codex_core=info,codex_tui=info,codex_rmcp_client=info,codex_realtime_webrtc=warn",
        )
    });
    Ok(Some(FileLogging {
        writer,
        filter,
        guard,
    }))
}

pub(super) fn install(
    prepared: Option<FileLogging>,
    feedback: &codex_feedback::CodexFeedback,
    log_db: Option<log_db::LogDbLayer>,
    otel: Option<&codex_otel::OtelProvider>,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let (tui_file_layer, guard) = match prepared {
        Some(FileLogging {
            writer,
            filter,
            guard,
        }) => (
            Some(
                tracing_subscriber::fmt::layer()
                    .with_writer(writer)
                    .with_target(true)
                    .with_ansi(false)
                    .with_span_events(
                        tracing_subscriber::fmt::format::FmtSpan::NEW
                            | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
                    )
                    .with_filter(filter),
            ),
            Some(guard),
        ),
        None => (None, None),
    };
    let feedback_layer = feedback.logger_layer();
    let feedback_metadata_layer = feedback.metadata_layer();
    let log_db_layer =
        log_db.map(|layer| layer.with_filter(alab_live_logs::filter(log_db::default_filter())));
    let otel_logger_layer = otel.and_then(|provider| provider.logger_layer());
    let otel_tracing_layer = otel.and_then(|provider| provider.tracing_layer());
    let _ = tracing_subscriber::registry()
        .with(tui_file_layer)
        .with(feedback_layer)
        .with(feedback_metadata_layer)
        .with(log_db_layer)
        .with(otel_logger_layer)
        .with(otel_tracing_layer)
        .try_init();
    guard
}

pub(super) fn process_metrics(
    otel: Option<&codex_otel::OtelProvider>,
    originator: &str,
) -> Option<codex_otel::MetricsClient> {
    let metrics = otel.and_then(codex_otel::OtelProvider::metrics).cloned();
    if let Some(metrics) = &metrics {
        let _ = codex_otel::record_process_start_once(metrics, originator);
        let recorder = codex_rollout::sqlite_telemetry_recorder(metrics.clone(), originator);
        let _ = codex_state::install_process_db_telemetry(recorder);
    }
    metrics
}

#[cfg(test)]
#[path = "owned_local_logging_tests.rs"]
mod tests;
