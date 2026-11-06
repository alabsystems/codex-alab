// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Data-only turn usage, retaining one sample per model across replacement.

use super::TurnTokenUsage;
use codex_otel::SessionTelemetry;
use codex_otel::TURN_TOKEN_USAGE_METRIC;
use codex_protocol::protocol::TokenUsage;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelUsage {
    model: String,
    usage: TokenUsage,
    tags: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    models: Vec<ModelUsage>,
}

#[derive(Default)]
pub(super) struct RestoredUsage(BTreeMap<String, ModelUsage>);

impl TurnTokenUsage {
    pub(crate) fn capture_exec_usage(&self) -> Result<serde_json::Value, String> {
        let mut models = self.alab_restored.0.clone();
        for (model, (telemetry, usage)) in &self.by_model {
            if models
                .insert(
                    model.clone(),
                    ModelUsage {
                        model: model.clone(),
                        usage: usage.clone(),
                        tags: telemetry.exec_metric_tags()?,
                    },
                )
                .is_some()
            {
                return Err("turn usage contains duplicate model buckets".into());
            }
        }
        let value = serde_json::to_value(Snapshot {
            version: 1,
            models: models.into_values().collect(),
        })
        .map_err(|_| "turn usage could not be encoded".to_owned())?;
        // Preflight capture must reject anything the replacement cannot read,
        // while the original runtime still owns its execution state.
        Self::restore_exec_usage(value.clone())?;
        Ok(value)
    }

    pub(crate) fn restore_exec_usage(value: serde_json::Value) -> Result<Self, String> {
        let snapshot: Snapshot =
            serde_json::from_value(value).map_err(|_| "invalid turn usage snapshot".to_owned())?;
        if snapshot.version != 1 {
            return Err("unsupported turn usage snapshot".into());
        }
        let mut models = BTreeMap::new();
        for model in snapshot.models {
            SessionTelemetry::validate_exec_metric_tags(&model.tags)?;
            if models.insert(model.model.clone(), model).is_some() {
                return Err("invalid turn usage attribution".into());
            }
        }
        Ok(Self {
            by_model: BTreeMap::new(),
            alab_restored: RestoredUsage(models),
        })
    }

    pub(super) fn record_exec_usage(&mut self, model: &str, usage: &TokenUsage) -> bool {
        let Some(saved) = self.alab_restored.0.get_mut(model) else {
            return false;
        };
        saved.usage.add_assign(usage);
        true
    }

    pub(super) fn emit_exec_usage(
        &mut self,
        sink: &SessionTelemetry,
        tmp_mem: (&str, &str),
    ) -> bool {
        let models = std::mem::take(&mut self.alab_restored.0);
        let emitted = !models.is_empty();
        for model in models.into_values() {
            for (token_type, value) in [
                ("total", model.usage.total_tokens),
                ("input", model.usage.input_tokens),
                ("cached_input", model.usage.cached_input()),
                ("cache_write_input", model.usage.cache_write_input_tokens),
                ("output", model.usage.output_tokens),
                ("reasoning_output", model.usage.reasoning_output_tokens),
            ] {
                let mut tags = model
                    .tags
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect::<Vec<_>>();
                tags.push(("token_type", token_type));
                tags.push(tmp_mem);
                sink.exec_histogram(TURN_TOKEN_USAGE_METRIC, value.max(0), &tags);
            }
        }
        emitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telemetry(model: &str) -> SessionTelemetry {
        SessionTelemetry::new(
            codex_protocol::ThreadId::new(),
            model,
            model,
            None,
            None,
            None,
            "codex_exec".into(),
            false,
            "test".into(),
            codex_protocol::protocol::SessionSource::Exec,
        )
    }

    #[test]
    fn repeated_replacement_combines_each_models_usage_without_relabeling_it() {
        let usage = |value| TokenUsage {
            input_tokens: value,
            total_tokens: value,
            ..Default::default()
        };
        let mut original = TurnTokenUsage::default();
        original.record("first", telemetry("first"), &usage(7));
        original.record("second", telemetry("second"), &usage(11));
        let before = original.capture_exec_usage().unwrap();
        let mut recovered = TurnTokenUsage::restore_exec_usage(before.clone()).unwrap();
        // Deliberately different attribution: a later contribution must keep
        // the original per-model bucket, as uninterrupted recording does.
        recovered.record("first", telemetry("replacement"), &usage(13));
        recovered.record("third", telemetry("third"), &usage(17));
        let after = recovered.capture_exec_usage().unwrap();
        let restored = TurnTokenUsage::restore_exec_usage(after.clone()).unwrap();
        assert_eq!(restored.capture_exec_usage().unwrap(), after);
        let models = after["models"].as_array().unwrap();
        assert_eq!(models.len(), 3);
        assert_eq!(models[0]["usage"]["total_tokens"], 20);
        assert_eq!(models[0]["tags"], before["models"][0]["tags"]);
        assert_eq!(models[1]["usage"]["total_tokens"], 11);
        assert_eq!(models[2]["usage"]["total_tokens"], 17);
        assert_eq!(original.capture_exec_usage().unwrap(), before);
    }

    #[test]
    fn recovered_usage_emits_once_to_the_new_sink_with_original_model_tags() {
        use opentelemetry_sdk::metrics::InMemoryMetricExporter;
        use opentelemetry_sdk::metrics::data::AggregatedMetrics;
        use opentelemetry_sdk::metrics::data::MetricData;

        let sink = telemetry("replacement")
            .with_metrics_config(
                codex_otel::MetricsConfig::in_memory(
                    "test",
                    "codex",
                    "test",
                    InMemoryMetricExporter::default(),
                )
                .with_runtime_reader(),
            )
            .unwrap();
        let usage = |value| TokenUsage {
            input_tokens: value,
            total_tokens: value,
            ..Default::default()
        };
        let mut original = TurnTokenUsage::default();
        original.record("original", telemetry("original"), &usage(7));
        let mut restored =
            TurnTokenUsage::restore_exec_usage(original.capture_exec_usage().unwrap()).unwrap();
        restored.record("original", sink.clone(), &usage(3));
        restored.emit(&sink, ("tmp_mem_enabled", "false"));

        let snapshot = sink.snapshot_metrics().unwrap();
        let metric = snapshot
            .scope_metrics()
            .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
            .find(|metric| metric.name() == TURN_TOKEN_USAGE_METRIC)
            .unwrap();
        let AggregatedMetrics::F64(MetricData::Histogram(histogram)) = metric.data() else {
            panic!("expected turn token histogram")
        };
        let points = histogram.data_points().collect::<Vec<_>>();
        assert_eq!(points.len(), 6, "no extra zero-valued replacement sample");
        let mut totals = BTreeMap::new();
        for point in points {
            let tags = point
                .attributes()
                .map(|tag| (tag.key.as_str(), tag.value.as_str().to_string()))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(tags["model"], "original");
            assert_eq!(point.count(), 1, "one sample per model and token category");
            totals.insert(tags["token_type"].clone(), point.sum());
        }
        assert_eq!(
            totals,
            BTreeMap::from([
                ("total".into(), 10.0),
                ("input".into(), 10.0),
                ("cached_input".into(), 0.0),
                ("cache_write_input".into(), 0.0),
                ("output".into(), 0.0),
                ("reasoning_output".into(), 0.0),
            ])
        );
    }

    #[test]
    fn invalid_version_duplicate_models_and_duplicate_tags_are_rejected() {
        let mut original = TurnTokenUsage::default();
        original.record("first", telemetry("first"), &TokenUsage::default());
        let value = original.capture_exec_usage().unwrap();
        let mut invalid = value.clone();
        invalid["version"] = 2.into();
        assert!(TurnTokenUsage::restore_exec_usage(invalid).is_err());
        let mut invalid = value.clone();
        let duplicate = invalid["models"][0].clone();
        invalid["models"].as_array_mut().unwrap().push(duplicate);
        assert!(TurnTokenUsage::restore_exec_usage(invalid).is_err());
        let mut invalid = value.clone();
        let duplicate = invalid["models"][0]["tags"][0].clone();
        invalid["models"][0]["tags"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(TurnTokenUsage::restore_exec_usage(invalid).is_err());
        let mut invalid = value.clone();
        invalid["models"][0]["tags"][0][0] = "unexpected".into();
        assert!(TurnTokenUsage::restore_exec_usage(invalid).is_err());
        let mut invalid = value;
        invalid["models"][0]["tags"][0][1] = "\n".into();
        assert!(TurnTokenUsage::restore_exec_usage(invalid).is_err());
    }
}
