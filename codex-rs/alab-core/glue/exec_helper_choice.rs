// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native tree data is re-admitted under fresh helper/account/model authority.
use super::HelperTurnAdmission;
use super::SavedProgramResolution;
use crate::agent::helper_daybreak::Enrollment;
use crate::agent::helper_daybreak::SavedIntent;
use crate::agent::helper_daybreak::eligible_owner;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_features::Feature;
use codex_protocol::openai_models::SavedDaybreakChoice;
use codex_protocol::protocol::SessionSource;
use serde::Deserialize;
use serde::Serialize;

const ACCOUNT_CHANGED: &str = "The account for this restored helper's saved choice has changed. Save a current choice or start a new helper.";

/// A portable preference value, separate from upstream's runtime-only enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Preference {
    On,
    Off,
    Unset,
}

impl From<SavedDaybreakChoice> for Preference {
    fn from(value: SavedDaybreakChoice) -> Self {
        match value {
            SavedDaybreakChoice::On => Self::On,
            SavedDaybreakChoice::Off => Self::Off,
            SavedDaybreakChoice::Unset => Self::Unset,
        }
    }
}

impl From<Preference> for SavedDaybreakChoice {
    fn from(value: Preference) -> Self {
        match value {
            Preference::On => Self::On,
            Preference::Off => Self::Off,
            Preference::Unset => Self::Unset,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Snapshot {
    Root,
    Helper {
        choice: Option<Result<Preference, String>>,
        owner: Option<serde_json::Value>,
    },
}

/// The implicit choice frozen for one original admitted turn, not a live default.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveChoice {
    choice: Result<Preference, String>,
    owner: Option<serde_json::Value>,
}

impl std::fmt::Debug for ActiveChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ActiveHelperChoice(<private>)")
    }
}

impl ActiveChoice {
    pub(crate) fn capture(turn: &TurnContext) -> Option<Self> {
        turn.extension_data.get::<SavedProgramResolution>()?;
        let intent = turn.extension_data.get::<SavedIntent>();
        Some(Self {
            choice: intent
                .as_ref()
                .map(|intent| intent.choice.clone().map(Preference::from))
                .unwrap_or_else(|| {
                    Err("Implicit helper selection lacks its admitted choice".into())
                }),
            owner: intent.and_then(|intent| intent.owner.alab_identity_checkpoint()),
        })
    }

    fn restore(&self, turn: &mut TurnContext, owner: codex_login::AuthOwner) {
        turn.cyber_access_program = None;
        let choice = if self
            .owner
            .as_ref()
            .is_some_and(|saved| owner.alab_matches_identity_checkpoint(saved))
        {
            self.choice.clone().map(SavedDaybreakChoice::from)
        } else {
            Err(ACCOUNT_CHANGED.into())
        };
        let resolution = choice
            .clone()
            .and_then(|choice| super::select_program(turn.model_info(), choice));
        let bound_owner = resolution.map(|program| {
            turn.cyber_access_program = program;
            owner.clone()
        });
        turn.extension_data
            .insert(SavedProgramResolution(bound_owner));
        turn.extension_data.insert(SavedIntent { choice, owner });
    }
}

struct PendingRestore(Snapshot);

impl Snapshot {
    pub(crate) fn capture(session: &Session) -> Option<Self> {
        match session
            .services
            .thread_extension_data
            .get::<Enrollment>()?
            .as_ref()
        {
            Enrollment::Root => Some(Self::Root),
            Enrollment::NativeHelper => {
                let intent = session.services.thread_extension_data.get::<SavedIntent>();
                Some(Self::Helper {
                    choice: intent
                        .as_ref()
                        .map(|intent| intent.choice.clone().map(Preference::from)),
                    owner: intent.and_then(|intent| intent.owner.alab_identity_checkpoint()),
                })
            }
        }
    }

    pub(crate) fn validate(&self, source: &SessionSource) -> Result<(), String> {
        let valid_role = match self {
            Self::Root => !source.is_non_root_agent(),
            Self::Helper { choice, owner } => {
                source.is_non_root_agent()
                    && (owner.is_none() || choice.is_some())
                    && owner.as_ref().is_none_or(serde_json::Value::is_object)
            }
        };
        valid_role
            .then_some(())
            .ok_or_else(|| "helper snapshot changes its native tree role".into())
    }

    /// The caller still supplies the existing receipt-bound native tree admission.
    pub(crate) fn stage(&self, init: &mut codex_extension_api::ExtensionDataInit) {
        init.insert(PendingRestore(self.clone()));
    }
}

pub(crate) async fn restore_pending(session: &Session, turn: &mut TurnContext) {
    let data = &session.services.thread_extension_data;
    if data.get::<codex_alab_core::quota::HeadlessExec>().is_none() {
        return;
    }
    let pending = data.get::<PendingRestore>();
    let recovery = turn.extension_data.get::<crate::ExecRecoveryOptions>();
    let active = recovery
        .as_ref()
        .filter(|saved| {
            saved.thread() == session.thread_id()
                && saved.turn() == turn.sub_id
                && turn
                    .extension_data
                    .get::<codex_alab_core::renewal::TurnAdmission>()
                    .is_some()
        })
        .and_then(|saved| saved.helper_choice());
    if pending.is_none() && active.is_none() {
        return;
    }
    if pending
        .as_ref()
        .is_some_and(|pending| matches!(&pending.0, Snapshot::Root))
    {
        data.insert(Enrollment::Root);
        data.remove::<PendingRestore>();
    }
    if !turn
        .config
        .features
        .enabled(Feature::DaybreakProgramPropagation)
        || turn.config.model_provider_id != codex_model_provider_info::OPENAI_PROVIDER_ID
        || !codex_model_provider::provider_uses_first_party_auth_path(turn.provider.info())
        || !turn
            .config
            .model_provider
            .supports_cyber_access_programs(&turn.config.model_provider_id)
    {
        return;
    }
    let Some(owner) = eligible_owner(session) else {
        // Never manufacture a credential owner, even to represent a refusal.
        if turn.cyber_access_program.is_none()
            && (active.is_some()
                || pending
                    .as_ref()
                    .is_some_and(|pending| matches!(&pending.0, Snapshot::Helper { .. })))
        {
            turn.extension_data
                .insert(SavedProgramResolution(Err(ACCOUNT_CHANGED.into())));
        }
        return;
    };
    if let Some(pending) = pending.as_ref()
        && let Snapshot::Helper {
            choice,
            owner: original,
        } = &pending.0
    {
        data.insert(Enrollment::NativeHelper);
        if let Some(choice) = choice {
            let choice = if original
                .as_ref()
                .is_some_and(|saved| owner.alab_matches_identity_checkpoint(saved))
            {
                choice.clone().map(SavedDaybreakChoice::from)
            } else {
                Err(ACCOUNT_CHANGED.into())
            };
            data.insert(SavedIntent {
                choice,
                owner: owner.clone(),
            });
        }
        // New turns use ordinary own-preference precedence over inherited data.
        HelperTurnAdmission(owner.clone())
            .capture_intent(session, turn)
            .await;
        data.remove::<PendingRestore>();
    }
    if let Some(active) = active {
        // An original turn never re-reads a preference toggled after admission.
        active.restore(turn, owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use codex_login::CodexAuth;
    use codex_protocol::openai_models::ModelAccessPrograms;
    use codex_protocol::turn_input::CyberAccessProgram;
    use std::sync::Arc;

    fn auth(account: &str) -> CodexAuth {
        let claims =
            serde_json::json!({"https://api.openai.com/auth": {"chatgpt_user_id": "helper-user"}});
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
        CodexAuth::from_external_chatgpt_tokens(
            &format!("header.{payload}.signature"),
            account,
            None,
        )
        .unwrap()
    }

    async fn fixture(
        account: &str,
        enabled: bool,
    ) -> (tempfile::TempDir, Arc<Session>, TurnContext) {
        let home = tempfile::tempdir().unwrap();
        let (session, turn, _) =
            crate::session::tests::make_session_and_context_with_auth_config_home_and_rx(
                auth(account),
                Vec::new(),
                home.path(),
                |config| {
                    if enabled {
                        config
                            .features
                            .enable(Feature::DaybreakProgramPropagation)
                            .unwrap();
                    } else {
                        config
                            .features
                            .disable(Feature::DaybreakProgramPropagation)
                            .unwrap();
                    }
                },
            )
            .await;
        session
            .services
            .thread_extension_data
            .insert(codex_alab_core::quota::HeadlessExec);
        let mut turn = Arc::try_unwrap(turn).unwrap();
        let settings = Arc::make_mut(&mut turn.initial_settings);
        Arc::make_mut(&mut settings.model_info).available_access_programs =
            Some(ModelAccessPrograms {
                cyber: vec![
                    CyberAccessProgram::Standard,
                    CyberAccessProgram::DaybreakBlue,
                    CyberAccessProgram::DaybreakRed,
                ],
            });
        (home, session, turn)
    }

    fn captured(session: &Session, choice: SavedDaybreakChoice) -> Snapshot {
        session
            .services
            .thread_extension_data
            .insert(Enrollment::NativeHelper);
        session.services.thread_extension_data.insert(SavedIntent {
            choice: Ok(choice),
            owner: eligible_owner(session).unwrap(),
        });
        let source = Snapshot::capture(session).unwrap();
        serde_json::from_value(serde_json::to_value(source).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn restored_helper_data_binds_a_fresh_owner_and_keeps_selection_implicit() {
        let (_source_home, source, _) = fixture("same-account", true).await;
        let old_owner = eligible_owner(&source).unwrap();
        let snapshot = captured(&source, SavedDaybreakChoice::On);
        let (_home, fresh, mut turn) = fixture("same-account", true).await;
        fresh
            .services
            .thread_extension_data
            .insert(PendingRestore(snapshot));
        restore_pending(&fresh, &mut turn).await;
        assert_eq!(
            turn.cyber_access_program,
            Some(CyberAccessProgram::DaybreakBlue)
        );
        turn.extension_data
            .get::<SavedProgramResolution>()
            .unwrap()
            .validate()
            .unwrap();
        let choice = turn.extension_data.get::<SavedIntent>().unwrap();
        assert_eq!(choice.choice, Ok(SavedDaybreakChoice::On));
        assert_eq!(choice.owner, eligible_owner(&fresh).unwrap());
        assert_ne!(
            choice.owner, old_owner,
            "old process authority is never revived"
        );
        assert!(
            fresh
                .services
                .thread_extension_data
                .get::<PendingRestore>()
                .is_none()
        );
    }

    #[tokio::test]
    async fn another_account_refuses_the_inherited_automatic_choice() {
        let (_source_home, source, _) = fixture("original-account", true).await;
        let snapshot = captured(&source, SavedDaybreakChoice::On);
        let (_home, fresh, mut turn) = fixture("destination-account", true).await;
        fresh
            .services
            .thread_extension_data
            .insert(PendingRestore(snapshot));
        restore_pending(&fresh, &mut turn).await;
        assert!(turn.cyber_access_program.is_none());
        assert!(
            turn.extension_data
                .get::<SavedProgramResolution>()
                .unwrap()
                .validate()
                .is_err()
        );
        assert!(
            turn.extension_data
                .get::<SavedIntent>()
                .unwrap()
                .choice
                .is_err()
        );
    }

    #[tokio::test]
    async fn an_explicit_override_does_not_borrow_an_inherited_choice() {
        let (_source_home, source, _) = fixture("original-account", true).await;
        let snapshot = captured(&source, SavedDaybreakChoice::On);
        let (_home, fresh, mut turn) = fixture("destination-account", true).await;
        turn.cyber_access_program = Some(CyberAccessProgram::DaybreakRed);
        fresh
            .services
            .thread_extension_data
            .insert(PendingRestore(snapshot));
        restore_pending(&fresh, &mut turn).await;
        assert_eq!(
            turn.cyber_access_program,
            Some(CyberAccessProgram::DaybreakRed)
        );
        assert!(
            turn.extension_data
                .get::<SavedProgramResolution>()
                .is_none()
        );
        assert!(
            turn.extension_data
                .get::<SavedIntent>()
                .unwrap()
                .choice
                .is_err()
        );
    }

    #[tokio::test]
    async fn stored_data_cannot_activate_disabled_propagation() {
        let (_source_home, source, _) = fixture("same-account", true).await;
        let snapshot = captured(&source, SavedDaybreakChoice::On);
        let (_home, fresh, mut turn) = fixture("same-account", false).await;
        fresh
            .services
            .thread_extension_data
            .insert(PendingRestore(snapshot));
        restore_pending(&fresh, &mut turn).await;
        assert!(turn.cyber_access_program.is_none());
        assert!(
            turn.extension_data
                .get::<SavedProgramResolution>()
                .is_none()
        );
        assert!(
            fresh
                .services
                .thread_extension_data
                .get::<Enrollment>()
                .is_none()
        );
    }

    #[tokio::test]
    async fn active_choice_is_frozen_separately_from_later_turn_defaults() {
        let (_source_home, source, mut old_turn) = fixture("same-account", true).await;
        let owner = eligible_owner(&source).unwrap();
        old_turn
            .extension_data
            .insert(SavedProgramResolution(Ok(owner.clone())));
        old_turn.extension_data.insert(SavedIntent {
            choice: Ok(SavedDaybreakChoice::On),
            owner,
        });
        old_turn.cyber_access_program = Some(CyberAccessProgram::DaybreakBlue);
        let active = ActiveChoice::capture(&old_turn).unwrap();
        let active: ActiveChoice =
            serde_json::from_value(serde_json::to_value(active).unwrap()).unwrap();
        let snapshot = captured(&source, SavedDaybreakChoice::Off);
        let (_home, fresh, mut turn) = fixture("same-account", true).await;
        let settings = Arc::make_mut(&mut turn.initial_settings);
        Arc::make_mut(&mut settings.model_info).available_access_programs =
            Some(ModelAccessPrograms {
                cyber: vec![
                    CyberAccessProgram::Standard,
                    CyberAccessProgram::DaybreakRed,
                ],
            });
        fresh
            .services
            .thread_extension_data
            .insert(PendingRestore(snapshot));
        restore_pending(&fresh, &mut turn).await;
        assert_eq!(
            turn.cyber_access_program,
            Some(CyberAccessProgram::Standard)
        );
        active.restore(&mut turn, eligible_owner(&fresh).unwrap());
        assert_eq!(
            turn.cyber_access_program,
            Some(CyberAccessProgram::DaybreakRed)
        );
        turn.extension_data
            .get::<SavedProgramResolution>()
            .unwrap()
            .validate()
            .unwrap();
        assert_eq!(
            turn.extension_data.get::<SavedIntent>().unwrap().choice,
            Ok(SavedDaybreakChoice::On)
        );
        assert_eq!(
            fresh
                .services
                .thread_extension_data
                .get::<SavedIntent>()
                .unwrap()
                .choice,
            Ok(SavedDaybreakChoice::Off)
        );
    }

    #[test]
    fn portable_preference_preserves_all_three_states_and_refuses_unknown_values() {
        for (choice, wire) in [
            (SavedDaybreakChoice::On, "on"),
            (SavedDaybreakChoice::Off, "off"),
            (SavedDaybreakChoice::Unset, "unset"),
        ] {
            let stored = serde_json::to_value(Preference::from(choice)).unwrap();
            assert_eq!(stored, wire);
            let restored: Preference = serde_json::from_value(stored).unwrap();
            assert_eq!(SavedDaybreakChoice::from(restored), choice);
        }
        assert!(serde_json::from_value::<Preference>(serde_json::json!("future-choice")).is_err());
    }

    #[test]
    fn a_child_checkpoint_cannot_reenroll_as_a_root() {
        let source = SessionSource::Internal(
            codex_protocol::protocol::InternalSessionSource::MemoryConsolidation,
        );
        assert!(Snapshot::Root.validate(&source).is_err());
        assert!(
            Snapshot::Helper {
                choice: None,
                owner: None
            }
            .validate(&SessionSource::Exec)
            .is_err()
        );
    }
}
