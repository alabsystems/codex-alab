// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve metric attribution while rebinding the sink in a replacement process.

use super::SessionTelemetry;

impl SessionTelemetry {
    pub fn exec_metric_tags(&self) -> Result<Vec<(String, String)>, String> {
        let tags: Vec<(String, String)> = self
            .metadata_tag_refs()
            .map(|tags| {
                tags.into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect()
            })
            .map_err(|_| "turn metric attribution could not be captured".to_owned())?;
        Self::validate_exec_metric_tags(&tags)?;
        Ok(tags)
    }

    pub fn validate_exec_metric_tags(tags: &[(String, String)]) -> Result<(), String> {
        let mut keys = std::collections::BTreeSet::new();
        for (key, value) in tags {
            if !matches!(
                key.as_str(),
                "auth_mode"
                    | "session_source"
                    | "originator"
                    | "service_name"
                    | "model"
                    | "app.version"
            ) || !keys.insert(key)
                || crate::metrics::validation::validate_tag_key(key).is_err()
                || crate::metrics::validation::validate_tag_value(value).is_err()
            {
                return Err("invalid turn metric attribution".into());
            }
        }
        Ok(())
    }

    /// These tags already include the original session attribution. Adding the
    /// replacement's metadata would relabel usage produced before replacement.
    pub fn exec_histogram(&self, name: &str, value: i64, tags: &[(&str, &str)]) {
        if let Some(metrics) = &self.metrics
            && let Err(error) = metrics.histogram(name, value, tags)
        {
            tracing::warn!("recovered metrics histogram [{name}] failed: {error}");
        }
    }
}
