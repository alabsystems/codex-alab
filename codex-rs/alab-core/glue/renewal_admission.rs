// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

pub(super) async fn admit_renewal_turn(
    session: &std::sync::Arc<super::Session>,
    context: &super::TurnContext,
) -> codex_protocol::error::Result<()> {
    if let Some(admission) = context
        .extension_data
        .get::<codex_alab_core::renewal::TurnAdmission>()
    {
        // This is startup-only data, not a setting for future ordinary turns.
        {
            let mut state = session.state.lock().await;
            if state
                .session_configuration
                .turn_extension_init
                .get::<codex_alab_core::renewal::TurnAdmission>()
                .is_some_and(|saved| std::sync::Arc::ptr_eq(&saved, &admission))
            {
                state
                    .session_configuration
                    .turn_extension_init
                    .remove::<codex_alab_core::renewal::TurnAdmission>();
            }
        }
        admission.admit(context.sub_id.clone()).await.map_err(|_| {
            codex_protocol::error::CodexErr::Fatal(
                "same-account renewal continuation was not durably admitted".into(),
            )
        })?;
    }
    crate::session::input_queue::exec_input_state::install_after_admission(session, context).await
}
