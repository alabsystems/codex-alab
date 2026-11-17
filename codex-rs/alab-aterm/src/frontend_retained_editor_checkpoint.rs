// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Closed editor selection DATA. Deserializing this cannot acquire a child,
//! document, terminal, reap owner, Native currentness, or UI acknowledgement.
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorCheckpoint {
    schema: u32,
    editor: String,
    document: String,
    generation: u64,
}
impl EditorCheckpoint {
    pub fn validate_data(&self) -> io::Result<()> {
        let token = |value: &str| {
            value.len() == 32
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if self.schema != 1 || !token(&self.editor) || !token(&self.document) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid editor checkpoint DATA",
            ));
        }
        Ok(())
    }
    pub fn editor_data(&self) -> &str {
        &self.editor
    }
    pub fn document_data(&self) -> &str {
        &self.document
    }
    pub fn generation_data(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_is_closed_data_not_a_pid_path_or_permission() -> Result<(), serde_json::Error> {
        let value = serde_json::json!({"schema":1,"editor":"a".repeat(32),"document":"b".repeat(32),"generation":9});
        let decoded: EditorCheckpoint = serde_json::from_value(value.clone())?;
        assert!(decoded.validate_data().is_ok());
        assert_eq!(decoded.generation_data(), 9);
        for field in ["pid", "path", "nativeCurrent", "reapOwner"] {
            let mut extra = value.clone();
            extra[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<EditorCheckpoint>(extra).is_err());
        }
        for (field, changed) in [
            ("schema", serde_json::json!(2)),
            ("editor", serde_json::json!("A".repeat(32))),
            ("document", serde_json::json!("c".repeat(31))),
        ] {
            let mut invalid = value.clone();
            invalid[field] = changed;
            let decoded: EditorCheckpoint = serde_json::from_value(invalid)?;
            assert!(decoded.validate_data().is_err());
        }
        Ok(())
    }
}
