// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One account pane's text and ownership of its latest requested snapshot.

use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct SnapshotToken(Arc<()>);

#[derive(Clone, Debug)]
pub enum Update {
    Snapshot {
        token: SnapshotToken,
        result: Result<String, String>,
    },
    Observation(String),
}

#[derive(Debug)]
pub struct View {
    text: String,
    snapshot: Option<String>,
    current: Option<SnapshotToken>,
}

impl View {
    pub fn new(text: String) -> Self {
        Self {
            text,
            snapshot: None,
            current: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Reserve before spawning so scheduling cannot reorder refresh ownership.
    pub fn begin_snapshot(&mut self) -> SnapshotToken {
        let token = SnapshotToken(Arc::new(()));
        self.current = Some(token.clone());
        token
    }

    /// Accept a snapshot once, or let assignment progress supersede pending reads.
    pub fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Snapshot { token, result } => {
                if !self
                    .current
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(&current.0, &token.0))
                {
                    return false;
                }
                self.current = None;
                match result {
                    Ok(text) => {
                        self.snapshot = Some(text.clone());
                        self.text = text;
                    }
                    Err(error) => {
                        self.text = match &self.snapshot {
                            Some(snapshot) => format!(
                                "{error}\n\nShowing previously loaded account information; it may be out of date.\n\n{snapshot}"
                            ),
                            None => error,
                        };
                    }
                }
            }
            Update::Observation(text) => {
                self.current = None;
                self.snapshot = None;
                self.text = text;
            }
        }
        true
    }
}
