// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original BottomPaneView variant DATA. Default unsupported variants preserve
//! their real C1 owners; they cannot disappear into an empty captured stack.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum ViewData {
    DelayedApproval {
        original: Box<serde_json::value::RawValue>,
    },
    Approval {
        original: Box<serde_json::value::RawValue>,
    },
    UserInput {
        original: Box<serde_json::value::RawValue>,
    },
}

// Deserialize the closed wire object directly: serde's internally tagged
// enum buffer cannot preserve RawValue's original JSON representation.
impl<'de> Deserialize<'de> for ViewData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Encoded {
            kind: String,
            original: Box<serde_json::value::RawValue>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        match encoded.kind.as_str() {
            "delayedApproval" => Ok(Self::DelayedApproval {
                original: encoded.original,
            }),
            "approval" => Ok(Self::Approval {
                original: encoded.original,
            }),
            "userInput" => Ok(Self::UserInput {
                original: encoded.original,
            }),
            _ => Err(serde::de::Error::unknown_variant(
                &encoded.kind,
                &["userInput", "approval", "delayedApproval"],
            )),
        }
    }
}

impl ViewData {
    /// Pure native editing DATA selection, not Source outcome/closure evidence.
    pub(crate) fn editing_retaining(
        &self,
        keep: &[bool],
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<Option<Self>> {
        match self {
            Self::DelayedApproval { original } => {
                if !keep.iter().any(|keep| *keep) {
                    return Ok(None);
                }
                Ok(Some(Self::DelayedApproval {
                    original:
                        crate::bottom_pane::alab_retained_delayed_approval::editing_retaining(
                            original, keep, max_bytes,
                        )?,
                }))
            }
            Self::Approval { original } => Ok(
                crate::bottom_pane::alab_retained_approval::editing_retaining(
                    original, keep, max_bytes,
                )?
                .map(|original| Self::Approval { original }),
            ),
            Self::UserInput { original } => Ok(
                super::request_user_input::alab_retained_view::retain_request_editing(
                    original,
                    keep,
                    max_bytes,
                    max_generations,
                )?
                .map(|original| Self::UserInput { original }),
            ),
        }
    }
    pub(crate) fn validate_native_data(
        &self,
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        match self {
            Self::DelayedApproval { original } => {
                drop(
                    crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(
                        original, max_bytes,
                    )?,
                );
                Ok(())
            }
            Self::Approval { original } => {
                drop(
                    crate::bottom_pane::alab_retained_approval::PreparedApproval::decode(
                        original, max_bytes,
                    )?,
                );
                Ok(())
            }
            Self::UserInput { original } => {
                drop(
                    super::request_user_input::alab_retained_view::PreparedUserInput::decode(
                        original,
                        max_bytes,
                        max_generations,
                    )?,
                );
                Ok(())
            }
        }
    }
}

impl BottomPane {
    /// Find the actual original variant by its WHOLE current request and saved
    /// pending queue prefix. A removed preceding view cannot shift an index
    /// into an unrelated modal, and new appended real requests stay in place.
    pub(crate) fn alab_install_matching_retained_view(
        &mut self,
        data: ViewData,
        requests: &[codex_app_server_protocol::ToolRequestUserInputParams],
        owners: &[crate::app::app_server_requests::alab_retained_owners::UserInputOwner<'_>],
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        let mut indices = self
            .view_stack
            .iter()
            .enumerate()
            .filter_map(|(index, view)| {
                view.alab_matches_retained_requests(requests)
                    .then_some(index)
            });
        let index = indices.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "actual replayed request variant is not installed",
            )
        })?;
        if indices.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "multiple original views match one callback editing graph",
            ));
        }
        self.alab_install_retained_view(index, data, owners, max_bytes, max_generations)
    }

    pub(crate) fn alab_capture_retained_view_stack(
        &self,
        max_bytes: usize,
    ) -> io::Result<Vec<ViewData>> {
        if !self.delayed_approval_requests.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "delayed approval owners require their original per-variant transfer",
            ));
        }
        self.view_stack
            .iter()
            .map(|view| view.alab_capture_retained_view(max_bytes))
            .collect()
    }

    /// One original view is installed synchronously under App's exclusive event
    /// ownership, after exact callback binding. App's whole-UI pending owner must
    /// retain which exact sections have installed through tail/End/paint/commit.
    pub(crate) fn alab_install_retained_view(
        &mut self,
        index: usize,
        data: ViewData,
        owners: &[crate::app::app_server_requests::alab_retained_owners::UserInputOwner<'_>],
        max_bytes: usize,
        max_generations: usize,
    ) -> io::Result<()> {
        let view = self.view_stack.get_mut(index).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "original replayed pending view is not installed",
            )
        })?;
        view.alab_install_retained_view(data, owners, &self.keymap, max_bytes, max_generations)
    }
}

#[path = "retained_bound_views.rs"]
pub(crate) mod bound;
