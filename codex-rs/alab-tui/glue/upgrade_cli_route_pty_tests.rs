// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Isolated CLI libtest destination for the same-PID PTY/lease transfer.
//! Only the native constructor is a fixture. It checks the exact original
//! frame and supplies no credential, activation or settlement operation.
use codex_alab_core::native_source as source;
use codex_alab_core::runtime_upgrade as runtime;
use sha2::Digest;
use sha2::Sha256;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

const PRIMARY: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Clone)]
struct Constructor {
    digest: String,
    loads: Arc<AtomicUsize>,
}

impl source::Host for Constructor {
    fn source<'a>(&'a self, _: &'a str) -> source::SourceFuture<'a> {
        panic!("CLI routing read credentials")
    }
    fn open_member<'a>(&'a self, _: &'a str) -> source::MemberFuture<'a> {
        Box::pin(async { Ok(Arc::new(self.clone()) as Arc<dyn source::Member>) })
    }
}

impl source::Member for Constructor {
    fn id(&self) -> &str {
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }
    fn live(&self) -> bool {
        true
    }
    fn preload<'a>(
        &'a self,
        _: &'a str,
        _: Option<&'a source::Borrow>,
    ) -> source::PreloadFuture<'a> {
        panic!("CLI routing preloaded credentials")
    }
    fn ready<'a>(
        &'a self,
        _: &'a str,
        _: Option<&'a str>,
        _: &'a source::Loaded,
    ) -> source::ReadyFuture<'a> {
        panic!("CLI routing requested a model grant")
    }
    fn retire<'a>(&'a self, _: Option<&'a str>) -> source::UnitFuture<'a> {
        panic!("CLI routing retired its constructor")
    }
    fn runtime_upgrade(&self) -> Option<Arc<dyn runtime::Host>> {
        Some(Arc::new(self.clone()))
    }
}

impl runtime::Host for Constructor {
    fn load<'a>(
        &'a self,
        primary: &'a str,
        bytes: &'a [u8],
        build: u64,
    ) -> runtime::LoadFuture<'a> {
        Box::pin(async move {
            assert_eq!(primary, PRIMARY);
            // The source custody fixture leases package build 7 before the same-PID exec.
            assert_eq!(build, 7);
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), self.digest);
            assert_eq!(self.loads.fetch_add(1, Ordering::SeqCst), 0);
            Ok(Arc::new(Unactivated) as Arc<dyn runtime::Continuation>)
        })
    }
}

struct Unactivated;
impl runtime::Continuation for Unactivated {
    fn check(&self) -> source::UnitFuture<'_> {
        panic!("CLI routing admitted a root")
    }
    fn activate(&self) -> source::UnitFuture<'_> {
        panic!("CLI routing activated a root")
    }
    fn settle(&self) -> source::UnitFuture<'_> {
        panic!("CLI routing settled the transfer")
    }
}

#[test]
#[ignore = "destination of the isolated exec custody PTY test"]
fn admitted_local_resume_child() -> anyhow::Result<()> {
    assert_eq!(
        std::env::var("ALAB_TEST_EXEC_CUSTODY_PID")?,
        std::process::id().to_string()
    );
    // SAFETY: this single selected child owns the inherited same-process
    // descriptors exclusively. It starts its Tokio runtime only after adoption.
    unsafe { codex_alab_tui::upgrade_exec_custody::adopt_before_threads()? };
    codex_alab_core::activate();
    let _recovery = super::stage()?;
    let pending = codex_alab_tui::upgrade_state::pending().expect("actual staged source frame");
    let original_deadline = pending.deadline();
    let loads = Arc::new(AtomicUsize::new(0));
    source::install(Arc::new(Constructor {
        digest: std::env::var("ALAB_TEST_CHECKPOINT_SHA256")?,
        loads: loads.clone(),
    }));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            codex_alab_core::native_source_launch::enter().await?;
            let route = |args: &[&str]| {
                super::parse(crate::config_override_args::try_parse_from(
                    args.iter().copied(),
                ))
            };
            for wrong in [
                vec!["codex", "resume", "00000000-0000-4000-8000-000000000002"],
                vec!["codex", "resume", PRIMARY, "new work"],
                vec!["codex", "--worktree", "resume", PRIMARY],
            ] {
                assert!(route(&wrong).await.is_err(), "{wrong:?}");
            }
            assert!(pending.continuation().is_err());
            assert_eq!(loads.load(Ordering::SeqCst), 0);
            let original = ["codex", "resume", PRIMARY];
            let cli = route(&original).await?;
            assert!(cli.interactive.no_daemon);
            assert_eq!(super::explicit_local_resume(&cli)?.to_string(), PRIMARY);
            assert_eq!(pending.deadline(), original_deadline);
            let admitted = pending.continuation()?;
            assert!(route(&original).await?.interactive.no_daemon);
            assert!(Arc::ptr_eq(&admitted, &pending.continuation()?));
            assert_eq!(loads.load(Ordering::SeqCst), 1);
            anyhow::Ok(())
        })?;
    println!("ALAB_ORIGINAL_RESUME_ROUTED");
    Ok(())
}
