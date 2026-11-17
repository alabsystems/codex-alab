// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Two exact image-tool native cells. Historical file names are inert UI DATA;
//! only an actual pre-ACK renderer binding admits restoration, never path access.
use super::*;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) enum Data {
    Viewed {
        activity_id: String,
        filename: String,
        path_label: String,
        #[serde(deserialize_with = "required")]
        link: Option<native::LinkData>,
    },
    Generated {
        failed: bool,
        lines: Vec<native::LineData>,
        #[serde(deserialize_with = "required")]
        saved_path: Option<Vec<u8>>,
    },
}
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        _shared: &mut native::Capture,
        _depth: usize,
    ) -> io::Result<Option<Self>> {
        if let Some(actual) = actual.as_any().downcast_ref::<ViewImageHistoryCell>() {
            return Ok(Some(Self::Viewed {
                activity_id: actual.activity_id.clone(),
                filename: actual.filename.clone(),
                path_label: actual.path_label.clone(),
                link: actual.link.as_ref().map(native::LinkData::capture),
            }));
        }
        Ok(actual
            .as_any()
            .downcast_ref::<ImageGenerationHistoryCell>()
            .map(|actual| Self::Generated {
                failed: actual.failed,
                lines: actual.lines.iter().map(native::LineData::capture).collect(),
                saved_path: actual
                    .saved_path
                    .as_ref()
                    .map(|path| path.as_path().as_os_str().as_bytes().to_vec()),
            }))
    }
    pub(in crate::history_cell) fn restore(
        self,
        _shared: &mut native::Restored,
        resources: &RestoreContext,
        _depth: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        resources.image_renderer()?.check()?;
        self.restore_data()
    }
    fn restore_data(self) -> io::Result<Box<dyn HistoryCell>> {
        Ok(match self {
            Self::Viewed {
                activity_id,
                filename,
                path_label,
                link,
            } => Box::new(ViewImageHistoryCell {
                activity_id,
                filename,
                path_label,
                link: link.as_ref().map(native::LinkData::restore).transpose()?,
            }),
            Self::Generated {
                failed,
                lines,
                saved_path,
            } => {
                if failed && saved_path.is_some() {
                    return Err(native::invalid(
                        "failed original generation cannot own a saved image path",
                    ));
                }
                let saved_path = saved_path
                    .map(|bytes| {
                        if bytes.contains(&0) || bytes.len() > 64 * 1024 {
                            return Err(native::invalid("image path DATA range"));
                        }
                        AbsolutePathBuf::try_from(PathBuf::from(std::ffi::OsString::from_vec(
                            bytes,
                        )))
                        .map_err(native::invalid)
                    })
                    .transpose()?;
                Box::new(ImageGenerationHistoryCell {
                    failed,
                    lines: lines
                        .iter()
                        .map(native::LineData::restore)
                        .collect::<io::Result<Vec<_>>>()?,
                    saved_path,
                })
            }
        })
    }
}
#[cfg(test)]
#[path = "retained_history_image_cells_tests.rs"]
mod tests;
