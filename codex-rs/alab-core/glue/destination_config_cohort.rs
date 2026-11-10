// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Configuration coalescing for an already captured participant set. The owning
//! resource actor must freeze membership and retain these futures through
//! cancellation; this helper spawns no tasks and grants no readiness/publication.
use super::*;

pub struct ParticipantPlan {
    source: Arc<AuthManager>,
    participants: Vec<Arc<dyn SessionParticipant>>,
    registration: Registration,
    plan: Arc<StagedPlan>,
}

/// Query every actual participant before beginning any backend work. A later
/// foreign registration cannot leave an earlier participant partially begun.
pub async fn preflight_begin(
    source: Arc<AuthManager>,
    participants: Vec<Arc<dyn SessionParticipant>>,
) -> io::Result<ParticipantPlan> {
    if participants.is_empty() {
        return Err(unavailable());
    }
    let mut registrations = Vec::with_capacity(participants.len());
    for participant in &participants {
        registrations.push(participant.registration().await.map_err(sanitized)?);
    }
    let registration = registrations.first().ok_or_else(unavailable)?.clone();
    for actual in &registrations {
        actual.check_source(&source)?;
        if !registration.same_occurrence(actual) {
            return Err(unavailable());
        }
    }
    // One occurrence, hence one B policy controller, is used by every session.
    let plan = participants[0]
        .begin(registration.clone())
        .await
        .map_err(sanitized)?;
    for actual in &registrations {
        actual.check_source(&source)?;
    }
    if !registration.same_occurrence(plan.registration()) {
        return Err(unavailable());
    }
    plan.backend.validate().map_err(sanitized)?;
    if !Arc::ptr_eq(plan.backend.source_manager(), &source) {
        return Err(unavailable());
    }
    Ok(ParticipantPlan {
        source,
        participants,
        registration,
        plan,
    })
}

impl ParticipantPlan {
    pub fn auth_config(&self) -> &AuthConfig {
        self.plan.auth_config()
    }

    pub fn validate(&self) -> io::Result<()> {
        self.registration.check_source(&self.source)?;
        if !self.registration.same_occurrence(self.plan.registration())
            || !Arc::ptr_eq(self.plan.backend.source_manager(), &self.source)
        {
            return Err(unavailable());
        }
        self.plan.backend.validate().map_err(sanitized)
    }

    /// One prepare per captured participant, using the SAME staged plan and
    /// cloned actual B context. There is no retry or detached cancellation task.
    pub async fn prepare(
        &self,
        context: AlabDestinationResourceContext,
    ) -> io::Result<PreparedConfigurations> {
        self.validate()?;
        context.current_binding().await?;
        let configurations = prepare_complete(
            &self.participants,
            &self.plan,
            &context,
            |participant, plan, context| async move {
                let prepared = participant
                    .prepare(plan, context.clone())
                    .await
                    .map_err(sanitized)?;
                prepared.validate(&context).await?;
                Ok(prepared)
            },
        )
        .await?;
        self.validate()?;
        let complete = PreparedConfigurations {
            configurations,
            context,
            source: self.source.clone(),
            registration: self.registration.clone(),
            plan: self.plan.clone(),
        };
        complete.validate().await?;
        Ok(complete)
    }
}

/// Retains every actual owner and the B context. A configuration collection is
/// not a resource-completion/native Ready/Applied proof or a publication gate.
pub struct PreparedConfigurations {
    configurations: Vec<PreparedConfig>,
    context: AlabDestinationResourceContext,
    source: Arc<AuthManager>,
    registration: Registration,
    plan: Arc<StagedPlan>,
}
impl PreparedConfigurations {
    pub fn configurations(&self) -> &[PreparedConfig] {
        &self.configurations
    }

    /// A preparation snapshot, not a batch publication gate. Actual resource
    /// publication must also hold a coordinated C gate across every owner; a
    /// sequence of these checks followed by an unlocked write is insufficient.
    pub async fn validate(&self) -> io::Result<()> {
        self.registration.check_source(&self.source)?;
        self.plan.backend.validate().map_err(sanitized)?;
        let current = self.context.current_binding().await?;
        let next = self
            .configurations
            .first()
            .ok_or_else(unavailable)?
            .next_registration();
        for prepared in &self.configurations {
            if !Arc::ptr_eq(prepared.manager(), self.context.manager())
                || prepared.binding() != &current
                || !next.same_occurrence(prepared.next_registration())
            {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "destination configuration collection changed",
                ));
            }
            prepared.validate(&self.context).await?;
        }
        // A legitimate refresh between participants requires a rebuild at the
        // real new binding. Never relabel old configurations with current C.
        if self.context.current_binding().await? != current {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "destination credential snapshot changed",
            ));
        }
        self.registration.check_source(&self.source)?;
        self.plan.backend.validate().map_err(sanitized)
    }
}

// The private accumulator is shared by actual SessionParticipant dispatch and
// metadata-only failure controls. It cannot mint a native B context or proof.
async fn prepare_complete<C: Clone, T, F, Fut>(
    participants: &[Arc<dyn SessionParticipant>],
    plan: &Arc<StagedPlan>,
    context: &C,
    mut prepare: F,
) -> io::Result<Vec<T>>
where
    F: FnMut(Arc<dyn SessionParticipant>, Arc<StagedPlan>, C) -> Fut,
    Fut: Future<Output = io::Result<T>>,
{
    let mut configurations = Vec::with_capacity(participants.len());
    for participant in participants {
        configurations.push(prepare(participant.clone(), plan.clone(), context.clone()).await?);
    }
    Ok(configurations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    struct Backend(Arc<AuthManager>);
    impl ConfigPreparer for Backend {
        fn source_manager(&self) -> &Arc<AuthManager> {
            &self.0
        }
        fn begin<'a>(&'a self, _: ThreadConfigSnapshot) -> BeginFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
    }
    struct Plan {
        source: Arc<AuthManager>,
        auth: AuthConfig,
    }
    impl ConfigPlan for Plan {
        fn source_manager(&self) -> &Arc<AuthManager> {
            &self.source
        }
        fn auth_config(&self) -> &AuthConfig {
            &self.auth
        }
        fn validate(&self) -> io::Result<()> {
            Ok(())
        }
        fn prepare<'a>(
            &'a self,
            _: AlabDestinationResourceContext,
            _: ThreadConfigSnapshot,
        ) -> PrepareFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
    }
    #[derive(Default)]
    struct Counts {
        registrations: AtomicUsize,
        begins: AtomicUsize,
        preparations: AtomicUsize,
    }
    struct Participant {
        registration: Registration,
        counts: Arc<Counts>,
        all_registrations: Arc<AtomicUsize>,
        expected: usize,
    }
    impl SessionParticipant for Participant {
        fn registration(&self) -> RegistrationFuture<'_> {
            Box::pin(async move {
                self.counts.registrations.fetch_add(1, Ordering::SeqCst);
                self.all_registrations.fetch_add(1, Ordering::SeqCst);
                Ok(self.registration.clone())
            })
        }
        fn begin<'a>(&'a self, registration: Registration) -> PlanFuture<'a> {
            Box::pin(async move {
                assert_eq!(
                    self.all_registrations.load(Ordering::SeqCst),
                    self.expected,
                    "every actual registration must be queried before effects"
                );
                self.counts.begins.fetch_add(1, Ordering::SeqCst);
                if !self.registration.same_occurrence(&registration) {
                    return Err(unavailable());
                }
                let source = registration.0.source.clone();
                let runtime = source.runtime_config();
                let (routes, _) = runtime.auth_route_config.alab_fresh_destination_routes()?;
                // Pre-effect configuration metadata only. This creates no B
                // ResourceContext, native installation, or PreparedConfig proof.
                let auth = AuthConfig {
                    codex_home: runtime.codex_home,
                    auth_credentials_store_mode: Default::default(),
                    keyring_backend_kind: Default::default(),
                    forced_login_method: None,
                    chatgpt_base_url: None,
                    forced_chatgpt_workspace_id: None,
                    managed_auth_policy: Default::default(),
                    auth_route_config: routes,
                };
                Ok(Arc::new(StagedPlan {
                    registration,
                    backend: Arc::new(Plan { source, auth }),
                }))
            })
        }
        fn prepare<'a>(
            &'a self,
            _: Arc<StagedPlan>,
            _: AlabDestinationResourceContext,
        ) -> ConfigurationFuture<'a> {
            Box::pin(async move {
                self.counts.preparations.fetch_add(1, Ordering::SeqCst);
                Err(unavailable())
            })
        }
    }
    fn source() -> Arc<AuthManager> {
        AuthManager::from_auth_for_testing(codex_login::CodexAuth::from_api_key("fixture-cohort"))
    }
    fn participants(
        registrations: Vec<Registration>,
    ) -> (Vec<Arc<dyn SessionParticipant>>, Vec<Arc<Counts>>) {
        let expected = registrations.len();
        let all_registrations = Arc::new(AtomicUsize::new(0));
        let mut counts = Vec::new();
        let participants = registrations
            .into_iter()
            .map(|registration| {
                let count = Arc::new(Counts::default());
                counts.push(count.clone());
                Arc::new(Participant {
                    registration,
                    counts: count,
                    all_registrations: all_registrations.clone(),
                    expected,
                }) as Arc<dyn SessionParticipant>
            })
            .collect();
        (participants, counts)
    }

    #[tokio::test]
    async fn late_foreign_backend_or_source_causes_zero_begin_and_zero_prepare() {
        for foreign_source in [false, true] {
            let actual = source();
            let registration = Registration::new(Arc::new(Backend(actual.clone()))).unwrap();
            let foreign = Registration::new(Arc::new(Backend(if foreign_source {
                source()
            } else {
                actual.clone()
            })))
            .unwrap();
            let (participants, counts) =
                participants(vec![registration.clone(), registration, foreign]);
            assert!(preflight_begin(actual, participants).await.is_err());
            for count in counts {
                assert_eq!(count.registrations.load(Ordering::SeqCst), 1);
                assert_eq!(count.begins.load(Ordering::SeqCst), 0);
                assert_eq!(count.preparations.load(Ordering::SeqCst), 0);
            }
        }
    }

    #[tokio::test]
    async fn same_actual_occurrence_begins_once_after_every_registration() {
        let actual = source();
        let backend: Arc<dyn ConfigPreparer> = Arc::new(Backend(actual.clone()));
        let registration = Registration::new(backend.clone()).unwrap();
        let separately_registered = Registration::new(backend).unwrap();
        let (participants, counts) = participants(vec![
            registration.clone(),
            separately_registered,
            registration,
        ]);
        let plan = preflight_begin(actual, participants).await.unwrap();
        plan.validate().unwrap();
        plan.auth_config().validate().unwrap();
        assert_eq!(
            counts
                .iter()
                .map(|count| count.begins.load(Ordering::SeqCst))
                .sum::<usize>(),
            1
        );
        assert_eq!(counts[0].begins.load(Ordering::SeqCst), 1);
        assert!(
            counts
                .iter()
                .all(|count| count.preparations.load(Ordering::SeqCst) == 0)
        );
    }

    #[tokio::test]
    async fn preparation_failure_returns_no_partial_collection_and_preserves_same_plan_context() {
        let actual = source();
        let registration = Registration::new(Arc::new(Backend(actual.clone()))).unwrap();
        let (participants, counts) = participants(vec![
            registration.clone(),
            registration.clone(),
            registration,
        ]);
        let plan = preflight_begin(actual, participants.clone()).await.unwrap();
        let expected_plan = plan.plan.clone();
        // Test the shared accumulator without inventing an actual native B
        // context. Metadata outputs are not PreparedConfig/Ready authority.
        let context = Arc::new(());
        let dropped = Arc::new(AtomicUsize::new(0));
        struct Metadata(Arc<AtomicUsize>);
        impl Drop for Metadata {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let result = prepare_complete(
            &participants,
            &plan.plan,
            &context,
            |participant, staged, captured| {
                assert!(Arc::ptr_eq(&staged, &expected_plan));
                assert!(Arc::ptr_eq(&captured, &context));
                let index = participants
                    .iter()
                    .position(|actual| Arc::ptr_eq(actual, &participant))
                    .unwrap();
                counts[index].preparations.fetch_add(1, Ordering::SeqCst);
                std::future::ready(if index == 1 {
                    Err(unavailable())
                } else {
                    Ok(Metadata(dropped.clone()))
                })
            },
        )
        .await;
        assert!(result.is_err(), "no partial output bundle may be returned");
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(counts[0].preparations.load(Ordering::SeqCst), 1);
        assert_eq!(counts[1].preparations.load(Ordering::SeqCst), 1);
        assert_eq!(counts[2].preparations.load(Ordering::SeqCst), 0);
        assert_eq!(
            counts
                .iter()
                .map(|count| count.begins.load(Ordering::SeqCst))
                .sum::<usize>(),
            1
        );
    }
}
