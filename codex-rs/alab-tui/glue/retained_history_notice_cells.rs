// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Historical notice fields remain native, without performing an update or policy action.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::borrow::Cow;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) enum Data {
    Update {
        latest_version: String,
        action: Option<u8>,
    },
    Safety {
        title: String,
        explanation: Option<String>,
        body: String,
        actions: Vec<(String, String)>,
    },
    Deprecation {
        summary: String,
        details: Option<String>,
    },
    RecapLoading {
        start_time: Snapshot,
        animations_enabled: bool,
    },
    Recap {
        recap: String,
        next_action: Option<String>,
    },
}
fn action(actual: UpdateAction) -> u8 {
    use crate::update_action::DaemonUpdateSource as D;
    use crate::update_action::UpdateAction as A;
    match actual {
        A::Daemon(D::PublicStable) => 0,
        A::Daemon(D::ThisCli) => 1,
        A::NpmGlobalLatest => 2,
        A::BunGlobalLatest => 3,
        A::VitePlusGlobalLatest => 4,
        A::PnpmGlobalLatest => 5,
        A::BrewUpgrade => 6,
        A::StandaloneUnix => 7,
        A::StandaloneWindows => 8,
    }
}
fn restore_action(value: u8) -> io::Result<UpdateAction> {
    use crate::update_action::DaemonUpdateSource as D;
    use crate::update_action::UpdateAction as A;
    Ok(match value {
        0 => A::Daemon(D::PublicStable),
        1 => A::Daemon(D::ThisCli),
        2 => A::NpmGlobalLatest,
        3 => A::BunGlobalLatest,
        4 => A::VitePlusGlobalLatest,
        5 => A::PnpmGlobalLatest,
        6 => A::BrewUpgrade,
        7 => A::StandaloneUnix,
        8 => A::StandaloneWindows,
        _ => return Err(native::invalid("unknown original update notice action")),
    })
}
impl Data {
    pub(in crate::history_cell) fn capture(actual: &dyn HistoryCell) -> io::Result<Option<Self>> {
        let data =
            if let Some(actual) = actual.as_any().downcast_ref::<UpdateAvailableHistoryCell>() {
                Self::Update {
                    latest_version: actual.latest_version.clone(),
                    action: actual.update_action.map(action),
                }
            } else if let Some(actual) = actual.as_any().downcast_ref::<SafetyAccessBlockCell>() {
                Self::Safety {
                    title: actual.title.to_string(),
                    explanation: actual.explanation.clone(),
                    body: actual.body.to_string(),
                    actions: actual
                        .actions
                        .iter()
                        .map(|(label, target)| (label.to_string(), target.to_string()))
                        .collect(),
                }
            } else if let Some(actual) = actual.as_any().downcast_ref::<DeprecationNoticeCell>() {
                Self::Deprecation {
                    summary: actual.summary.clone(),
                    details: actual.details.clone(),
                }
            } else if let Some(actual) = actual.as_any().downcast_ref::<ThreadRecapLoadingCell>() {
                Self::RecapLoading {
                    start_time: Snapshot::capture(actual.start_time).map_err(native::invalid)?,
                    animations_enabled: actual.animations_enabled,
                }
            } else if let Some(actual) = actual.as_any().downcast_ref::<ThreadRecapHistoryCell>() {
                Self::Recap {
                    recap: actual.recap.clone(),
                    next_action: actual.next_action.clone(),
                }
            } else {
                return Ok(None);
            };
        Ok(Some(data))
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<Box<dyn HistoryCell>> {
        Ok(match self {
            Self::Update {
                latest_version,
                action,
            } => Box::new(UpdateAvailableHistoryCell {
                latest_version,
                update_action: action.map(restore_action).transpose()?,
            }),
            Self::Safety {
                title,
                explanation,
                body,
                actions,
            } => Box::new(SafetyAccessBlockCell {
                title: Cow::Owned(title),
                explanation,
                body: Cow::Owned(body),
                actions: actions
                    .into_iter()
                    .map(|(label, target)| (Cow::Owned(label), Cow::Owned(target)))
                    .collect(),
            }),
            Self::Deprecation { summary, details } => {
                Box::new(DeprecationNoticeCell { summary, details })
            }
            Self::RecapLoading {
                start_time,
                animations_enabled,
            } => Box::new(ThreadRecapLoadingCell {
                start_time: start_time.restore().map_err(native::invalid)?,
                animations_enabled,
            }),
            Self::Recap { recap, next_action } => {
                Box::new(ThreadRecapHistoryCell { recap, next_action })
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn roundtrip(actual: &dyn HistoryCell) -> io::Result<Box<dyn HistoryCell>> {
        let data = Data::capture(actual)?
            .ok_or_else(|| native::invalid("actual notice variant missing"))?;
        serde_json::from_slice::<Data>(&serde_json::to_vec(&data).map_err(native::invalid)?)
            .map_err(native::invalid)?
            .restore()
    }
    #[test]
    fn every_native_notice_variant_retains_its_original_fields_and_views() -> io::Result<()> {
        let originals: Vec<Box<dyn HistoryCell>> = vec![
            Box::new(UpdateAvailableHistoryCell::new(
                "original version".into(),
                Some(UpdateAction::StandaloneUnix),
            )),
            Box::new(new_safety_access_block_event()),
            Box::new(new_cyber_policy_error_event(crate::daybreak::Notice::Apply)),
            Box::new(new_deprecation_notice(
                "exact summary".into(),
                Some("exact original details".into()),
            )),
            Box::new(ThreadRecapLoadingCell::new(false)),
            Box::new(
                ThreadRecapHistoryCell::new("actual recap\n- original list".into())
                    .with_next_action(Some("exact next action".into())),
            ),
        ];
        for original in originals {
            let restored = roundtrip(original.as_ref())?;
            assert_eq!(original.as_any().type_id(), restored.as_any().type_id());
            for width in [0, 5, 24, 97] {
                assert_eq!(original.display_lines(width), restored.display_lines(width));
                assert_eq!(
                    original.transcript_lines(width),
                    restored.transcript_lines(width)
                );
            }
            assert_eq!(original.raw_lines(), restored.raw_lines());
        }
        Ok(())
    }
    #[test]
    fn historical_safety_strings_and_actions_are_owned_without_static_leaks_or_current_policy_substitution()
    -> io::Result<()> {
        let restored = Data::Safety {
            title: "historical title".into(),
            explanation: Some("historical explanation".into()),
            body: "historical body".into(),
            actions: vec![(
                "historical label".into(),
                "https://example.invalid/historical".into(),
            )],
        }
        .restore()?;
        let actual = restored
            .as_any()
            .downcast_ref::<SafetyAccessBlockCell>()
            .ok_or_else(|| native::invalid("native safety type changed"))?;
        assert!(matches!(&actual.title, Cow::Owned(_)));
        assert!(matches!(&actual.body, Cow::Owned(_)));
        assert_eq!(actual.actions[0].0, "historical label");
        assert_eq!(actual.actions[0].1, "https://example.invalid/historical");
        assert!(restore_action(255).is_err());
        Ok(())
    }
}
