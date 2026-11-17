// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Completed transcript DATA only. No live form/reply or publication is created.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OptionData {
    label: String,
    description: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuestionData {
    id: String,
    header: String,
    question: String,
    is_other: bool,
    is_secret: bool,
    options: Option<Vec<OptionData>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    schema: u8,
    questions: Vec<QuestionData>,
    answers: BTreeMap<String, Vec<String>>,
    interrupted: bool,
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        _graph: &mut native::Capture,
        depth: usize,
    ) -> io::Result<Option<Self>> {
        native::depth(depth)?;
        let Some(actual) = actual.as_any().downcast_ref::<RequestUserInputResultCell>() else {
            return Ok(None);
        };
        Ok(Some(Self {
            schema: 1,
            questions: actual
                .questions
                .iter()
                .map(|question| QuestionData {
                    id: question.id.clone(),
                    header: question.header.clone(),
                    question: question.question.clone(),
                    is_other: question.is_other,
                    is_secret: question.is_secret,
                    options: question.options.as_ref().map(|options| {
                        options
                            .iter()
                            .map(|option| OptionData {
                                label: option.label.clone(),
                                description: option.description.clone(),
                            })
                            .collect()
                    }),
                })
                .collect(),
            answers: actual
                .answers
                .iter()
                .map(|(id, answer)| (id.clone(), answer.answers.clone()))
                .collect(),
            interrupted: actual.interrupted,
        }))
    }
    pub(in crate::history_cell) fn restore(
        self,
        _graph: &mut native::Restored,
        depth: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        native::depth(depth)?;
        if self.schema != 1
            || self.questions.len() > native::MAX_NODES
            || self.answers.len() > native::MAX_NODES
        {
            return Err(native::invalid("completed native question schema/capacity"));
        }
        Ok(Box::new(RequestUserInputResultCell {
            questions: self
                .questions
                .into_iter()
                .map(|question| ToolRequestUserInputQuestion {
                    id: question.id,
                    header: question.header,
                    question: question.question,
                    is_other: question.is_other,
                    is_secret: question.is_secret,
                    options: question.options.map(|options| {
                        options
                            .into_iter()
                            .map(
                                |option| codex_app_server_protocol::ToolRequestUserInputOption {
                                    label: option.label,
                                    description: option.description,
                                },
                            )
                            .collect()
                    }),
                })
                .collect(),
            answers: self
                .answers
                .into_iter()
                .map(|(id, answers)| (id, ToolRequestUserInputAnswer { answers }))
                .collect(),
            interrupted: self.interrupted,
        }))
    }
}
