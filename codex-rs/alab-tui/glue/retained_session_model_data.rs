// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native catalog DATA retains the native field deliberately skipped by the
//! protocol serializer. No model selection/request/Source authority is created.
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::protocol::MultiAgentVersion;
use serde::Deserialize;
use serde::Serialize;
use std::io;
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    native: ModelPreset,
    #[serde(deserialize_with = "required")]
    multi_agent: Option<u8>,
}
impl Data {
    pub(crate) fn capture(original: &ModelPreset) -> Self {
        let mut native = original.clone();
        let multi_agent = native.multi_agent_version.take().map(|value| match value {
            MultiAgentVersion::Disabled => 0,
            MultiAgentVersion::V1 => 1,
            MultiAgentVersion::V2 => 2,
        });
        Self {
            native,
            multi_agent,
        }
    }
    pub(crate) fn prepare(&self) -> io::Result<ModelPreset> {
        let mut native = self.native.clone();
        native.multi_agent_version = self
            .multi_agent
            .map(|value| match value {
                0 => Ok(MultiAgentVersion::Disabled),
                1 => Ok(MultiAgentVersion::V1),
                2 => Ok(MultiAgentVersion::V2),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "native catalog multi-agent state",
                )),
            })
            .transpose()?;
        Ok(native)
    }
}
