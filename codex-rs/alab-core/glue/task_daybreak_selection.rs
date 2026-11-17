// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Existing pure Daybreak selection, shared by TUI and backend host modules.
use codex_protocol::openai_models::ModelAccessPrograms;
use codex_protocol::turn_input::CyberAccessProgram;

pub(crate) fn program_for_turn(
    programs: Option<&ModelAccessPrograms>,
    model: &str,
    eligible_account: bool,
    enabled: bool,
) -> Result<Option<CyberAccessProgram>, String> {
    if !eligible_account {
        return if enabled {
            Err("Daybreak requires the OpenAI provider with either ChatGPT sign-in or an API key with Daybreak support enabled. Turn it off to continue.".into())
        } else {
            Ok(None)
        };
    }
    let Some(programs) = programs.filter(|programs| !programs.cyber.is_empty()) else {
        return Ok(None);
    };
    if enabled {
        programs.daybreak().map(Some).ok_or_else(|| format!(
            "Daybreak support for model {model} could not be confirmed by the connected server. Use /daybreak to turn it off, or choose a compatible model and server."
        ))
    } else {
        Ok(programs.standard())
    }
}
