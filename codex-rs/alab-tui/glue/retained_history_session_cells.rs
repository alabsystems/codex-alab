// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Historical native session presentation only; no configuration or file access.
use super::*;
use crate::history_cell::alab_retained_cells as cells;
use crate::history_cell::base::alab_retained::CompositeData;
use crate::history_cell::base::alab_retained::PlainData;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::history_cell) struct PathData(Vec<u8>);
impl PathData {
    fn capture(actual: &Path) -> Self {
        Self(actual.as_os_str().as_bytes().to_vec())
    }
    fn restore(self) -> io::Result<PathBuf> {
        if self.0.contains(&0) {
            return Err(native::invalid("historical session path contains NUL"));
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(self.0)))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::history_cell) struct HeaderData {
    version: String,
    model: String,
    reasoning_effort: Option<ReasoningEffortConfig>,
    directory: PathData,
    yolo_mode: bool,
}
impl HeaderData {
    fn capture(actual: &SessionHeaderHistoryCell) -> Self {
        Self {
            version: actual.version.to_string(),
            model: actual.model.clone(),
            reasoning_effort: actual.reasoning_effort.clone(),
            directory: PathData::capture(&actual.directory),
            yolo_mode: actual.yolo_mode,
        }
    }
    fn restore(self) -> io::Result<SessionHeaderHistoryCell> {
        Ok(SessionHeaderHistoryCell {
            version: std::borrow::Cow::Owned(self.version),
            model: self.model,
            reasoning_effort: self.reasoning_effort,
            directory: self.directory.restore()?,
            yolo_mode: self.yolo_mode,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) enum Data {
    Tooltip { tip: String, cwd: PathData },
    Notice { original: PlainData },
    Forked { original: PlainData },
    Info { original: CompositeData },
    Clear { original: HeaderData },
    Header { original: HeaderData },
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        context: &mut cells::CaptureContext<'_>,
        nesting: usize,
    ) -> io::Result<Option<Self>> {
        cells::depth(nesting)?;
        let data = if let Some(actual) = actual.as_any().downcast_ref::<TooltipHistoryCell>() {
            Self::Tooltip {
                tip: actual.tip.clone(),
                cwd: PathData::capture(&actual.cwd),
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<SessionNoticeCell>() {
            Self::Notice {
                original: PlainData::capture(&actual.0),
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<ForkedThreadHistoryCell>() {
            Self::Forked {
                original: PlainData::capture(&actual.0),
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<SessionInfoCell>() {
            Self::Info {
                original: CompositeData::capture(&actual.0, context, nesting)?,
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<ClearScreenHeader>() {
            Self::Clear {
                original: HeaderData::capture(&actual.0),
            }
        } else if let Some(actual) = actual.as_any().downcast_ref::<SessionHeaderHistoryCell>() {
            Self::Header {
                original: HeaderData::capture(actual),
            }
        } else {
            return Ok(None);
        };
        Ok(Some(data))
    }
    pub(in crate::history_cell) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut cells::RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        cells::depth(nesting)?;
        Ok(match self {
            Self::Tooltip { tip, cwd } => Box::new(TooltipHistoryCell {
                tip,
                cwd: cwd.restore()?,
            }),
            Self::Notice { original } => Box::new(SessionNoticeCell(original.restore()?)),
            Self::Forked { original } => Box::new(ForkedThreadHistoryCell(original.restore()?)),
            Self::Info { original } => Box::new(SessionInfoCell(
                original.restore(shared, resources, nesting)?,
            )),
            Self::Clear { original } => Box::new(ClearScreenHeader(original.restore()?)),
            Self::Header { original } => Box::new(original.restore()?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn original_header() -> SessionHeaderHistoryCell {
        SessionHeaderHistoryCell::new(
            "original model".into(),
            Some(ReasoningEffortConfig::Custom("native effort".into())),
            PathBuf::from(std::ffi::OsString::from_vec(b"/original/work\xff".to_vec())),
            "old-native-build",
        )
        .with_yolo_mode(true)
    }
    #[test]
    fn native_header_owns_exact_historical_version_effort_and_non_utf8_directory() -> io::Result<()>
    {
        let original = original_header();
        let raw = serde_json::to_vec(&HeaderData::capture(&original)).map_err(native::invalid)?;
        let restored = serde_json::from_slice::<HeaderData>(&raw)
            .map_err(native::invalid)?
            .restore()?;
        assert!(matches!(&restored.version, std::borrow::Cow::Owned(_)));
        assert_eq!(restored.version, original.version);
        assert_eq!(restored.model, original.model);
        assert_eq!(restored.reasoning_effort, original.reasoning_effort);
        assert_eq!(
            restored.directory.as_os_str().as_bytes(),
            original.directory.as_os_str().as_bytes()
        );
        assert_eq!(restored.yolo_mode, original.yolo_mode);
        for width in [0, 12, 80] {
            assert_eq!(restored.display_lines(width), original.display_lines(width));
        }
        Ok(())
    }
    #[test]
    fn restored_clear_header_keeps_original_transcript_omission_policy() -> io::Result<()> {
        let original = ClearScreenHeader(original_header());
        let restored = ClearScreenHeader(HeaderData::capture(&original.0).restore()?);
        assert!(
            (&restored as &dyn HistoryCell)
                .as_any()
                .is::<ClearScreenHeader>()
        );
        assert!(restored.raw_lines().is_empty());
        assert!(restored.transcript_lines(80).is_empty());
        assert_eq!(restored.live_raw_lines(), original.live_raw_lines());
        Ok(())
    }
    #[test]
    fn historical_directory_data_never_accepts_nul_or_opens_a_file() -> io::Result<()> {
        assert!(PathData(b"/bad\0path".to_vec()).restore().is_err());
        let original = PathBuf::from("/this/path/does/not/need/to/exist");
        assert_eq!(PathData::capture(&original).restore()?, original);
        Ok(())
    }
}
