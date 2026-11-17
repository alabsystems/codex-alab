// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native user-history DATA, including presentation and original cwd.
//! This reconstructs no request, sender, Core, filesystem permission or SDK role.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct UserData {
    schema: u8,
    message: String,
    text_elements: Vec<TextElement>,
    local_image_paths: Vec<PathData>,
    remote_image_urls: Vec<String>,
    presentation: Presentation,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Presentation {
    Typed {},
    Spoken {},
    AsyncReplies { replies: Vec<Reply>, cwd: PathData },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply {
    question_item_id: String,
    question: String,
    answer: String,
}
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct PathData(Vec<u8>);
impl PathData {
    fn capture(path: &Path) -> Self {
        Self(path.as_os_str().as_bytes().to_vec())
    }
    fn restore(self) -> std::io::Result<PathBuf> {
        if self.0.contains(&0) {
            return Err(std::io::Error::other("native user-cell path contains NUL"));
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(self.0)))
    }
}
impl UserData {
    pub(in crate::history_cell) fn capture(actual: &UserHistoryCell) -> Self {
        let presentation = match &actual.presentation {
            UserMessagePresentation::Typed => Presentation::Typed {},
            UserMessagePresentation::Spoken => Presentation::Spoken {},
            UserMessagePresentation::AsyncReplies { replies, cwd } => Presentation::AsyncReplies {
                replies: replies
                    .iter()
                    .map(|reply| Reply {
                        question_item_id: reply.question_item_id.clone(),
                        question: reply.question.clone(),
                        answer: reply.answer.clone(),
                    })
                    .collect(),
                cwd: PathData::capture(cwd),
            },
        };
        Self {
            schema: 1,
            message: actual.message.clone(),
            text_elements: actual.text_elements.clone(),
            local_image_paths: actual
                .local_image_paths
                .iter()
                .map(|path| PathData::capture(path))
                .collect(),
            remote_image_urls: actual.remote_image_urls.clone(),
            presentation,
        }
    }
    pub(in crate::history_cell) fn restore(self) -> std::io::Result<UserHistoryCell> {
        if self.schema != 1 {
            return Err(std::io::Error::other(
                "user-history native DATA version differs",
            ));
        }
        let presentation = match self.presentation {
            Presentation::Typed {} => UserMessagePresentation::Typed,
            Presentation::Spoken {} => UserMessagePresentation::Spoken,
            Presentation::AsyncReplies { replies, cwd } => UserMessagePresentation::AsyncReplies {
                replies: replies
                    .into_iter()
                    .map(|reply| AsyncQuestionReply {
                        question_item_id: reply.question_item_id,
                        question: reply.question,
                        answer: reply.answer,
                    })
                    .collect(),
                cwd: cwd.restore()?,
            },
        };
        Ok(UserHistoryCell {
            message: self.message,
            text_elements: self.text_elements,
            local_image_paths: self
                .local_image_paths
                .into_iter()
                .map(PathData::restore)
                .collect::<std::io::Result<Vec<_>>>()?,
            remote_image_urls: self.remote_image_urls,
            presentation,
        })
    }
}
