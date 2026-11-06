// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Restore volatile identities only after the caller validates the captured
//! tree and its replacement owner. This grants no turn execution authority.
use crate::agent::registry::AgentRegistry;
use crate::agent::types::AgentMetadata;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use std::sync::Arc;

pub(super) fn restore(
    registry: &Arc<AgentRegistry>,
    thread: ThreadId,
    source: &SessionSource,
    ephemeral: bool,
) -> Result<(), String> {
    let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        agent_path,
        agent_nickname,
        agent_role,
        ..
    }) = source
    else {
        return Err("captured child has no public agent identity".into());
    };
    if let Some(known) = registry.agent_metadata_for_thread(thread) {
        if known.agent_path != *agent_path
            || known.agent_nickname != *agent_nickname
            || known.agent_role != *agent_role
        {
            return Err("captured child conflicts with its registered identity".into());
        }
        return Ok(());
    }
    if !ephemeral {
        return Err("persisted child has no registered identity".into());
    }
    // A memory-only root has no stored descendant metadata to reload. Reserve
    // its captured path before committing so another child cannot be displaced.
    let mut reservation = registry
        .reserve_spawn_slot(None)
        .map_err(|error| error.to_string())?;
    if let Some(path) = agent_path {
        reservation
            .reserve_agent_path(path)
            .map_err(|error| error.to_string())?;
    }
    if let Some(nickname) = agent_nickname {
        reservation
            .reserve_agent_nickname_with_preference(&[], Some(nickname))
            .map_err(|error| error.to_string())?;
    }
    reservation.commit(AgentMetadata {
        agent_id: Some(thread),
        agent_path: agent_path.clone(),
        agent_nickname: agent_nickname.clone(),
        agent_role: agent_role.clone(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::AgentPath;

    fn source() -> SessionSource {
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id: ThreadId::new(),
            depth: 1,
            agent_path: Some(AgentPath::root().join("worker").unwrap()),
            agent_nickname: Some("Captured".into()),
            agent_role: Some("default".into()),
        })
    }

    #[test]
    fn memory_restore_is_exact_idempotent_and_rejects_path_collision() {
        let registry = Arc::new(AgentRegistry::default());
        let thread = ThreadId::new();
        let source = source();
        restore(&registry, thread, &source, true).unwrap();
        restore(&registry, thread, &source, true).unwrap();
        assert_eq!(registry.live_agents().len(), 1);
        assert!(restore(&registry, ThreadId::new(), &source, true).is_err());
        let known = registry.agent_metadata_for_thread(thread).unwrap();
        assert_eq!(known.agent_id, Some(thread));
        assert_eq!(known.agent_path, source.get_agent_path());
        assert_eq!(known.agent_nickname, source.get_nickname());
        assert_eq!(known.agent_role, source.get_agent_role());
    }

    #[test]
    fn durable_identity_must_exist_and_existing_identity_must_match() {
        let registry = Arc::new(AgentRegistry::default());
        let thread = ThreadId::new();
        let mut source = source();
        assert!(restore(&registry, thread, &source, false).is_err());
        assert!(registry.live_agents().is_empty());
        restore(&registry, thread, &source, true).unwrap();
        let SessionSource::SubAgent(SubAgentSource::ThreadSpawn { agent_role, .. }) = &mut source
        else {
            unreachable!();
        };
        *agent_role = Some("changed".into());
        assert!(restore(&registry, thread, &source, true).is_err());
        assert_eq!(
            registry
                .agent_metadata_for_thread(thread)
                .unwrap()
                .agent_role
                .as_deref(),
            Some("default")
        );
    }
}
