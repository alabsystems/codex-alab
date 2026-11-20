// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use clap::Subcommand;
use codex_alab_release::Release;
use codex_alab_release::Result;
use std::path::PathBuf;

#[cfg(unix)]
mod outer;
#[cfg(unix)]
mod predecessor;
#[cfg(unix)]
mod stage_status;
#[cfg(unix)]
use codex_alab_release::space;

#[cfg(unix)]
#[derive(serde::Serialize, serde::Deserialize)]
enum CompilerStage {
    Product {
        report: PathBuf,
        inputs: codex_alab_release::product::Inputs,
        upstream: codex_alab_release::product::Upstream,
    },
    Predecessor {
        inputs: codex_alab_release::product::Inputs,
        candidate: predecessor::Candidate,
    },
}

#[cfg(unix)]
impl CompilerStage {
    fn inputs(&self) -> &codex_alab_release::product::Inputs {
        match self {
            Self::Product { inputs, .. } | Self::Predecessor { inputs, .. } => inputs,
        }
    }
}

#[cfg(unix)]
#[derive(clap::Args)]
struct ProtectedArgs {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    cargo_home: PathBuf,
    #[arg(long)]
    read: Vec<PathBuf>,
    #[arg(long)]
    write: Vec<PathBuf>,
}

#[derive(Parser)]
#[command(about = "Native codex-alab release stages")]
struct Args {
    #[cfg(unix)]
    #[arg(long, hide = true)]
    release_owner_fd: Option<i32>,
    #[command(subcommand)]
    command: Action,
}

#[cfg(unix)]
#[derive(clap::Args)]
struct PublishArgs {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    intent: PathBuf,
    #[arg(long)]
    publisher: PathBuf,
    #[arg(long)]
    state: PathBuf,
    #[arg(long)]
    cargo_home: PathBuf,
    #[arg(long)]
    v8_cache: PathBuf,
    #[arg(long)]
    build: u64,
}

#[cfg(unix)]
#[derive(clap::Args)]
struct CycleArgs {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    publisher: PathBuf,
    #[arg(long)]
    state: PathBuf,
    #[arg(long)]
    cargo_home: PathBuf,
    #[arg(long)]
    v8_cache: PathBuf,
}

#[derive(Subcommand)]
enum Action {
    /// Build, assemble and publish one frozen regular release through native atpkg.
    #[cfg(unix)]
    Publish(PublishArgs),
    /// Discover or resume the registered regular release on the publishing worker.
    #[cfg(unix)]
    Cycle(CycleArgs),
    /// Run the released predecessor's target reader against an exact candidate.
    #[cfg(unix)]
    CheckPredecessor(predecessor::Args),
    /// Exercise the real macOS sandbox from a driver with no inherited sandbox.
    #[cfg(target_os = "macos")]
    VerifySandbox,
    /// Build source-pinned ripgrep with the same admitted Trust compiler as the product.
    #[cfg(unix)]
    Ripgrep {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        intent: PathBuf,
        #[arg(long)]
        observation: PathBuf,
        #[arg(long)]
        publisher: PathBuf,
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        cargo_home: PathBuf,
    },
    /// Assemble a create-only layout-v1 package from observed canonical outputs.
    #[cfg(unix)]
    Assemble {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        intent: PathBuf,
        #[arg(long)]
        observation: PathBuf,
        #[arg(long)]
        ripgrep_observation: PathBuf,
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build the canonical three executables from a frozen, leased source checkout.
    #[cfg(unix)]
    Build {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        intent: PathBuf,
        #[arg(long)]
        publisher: PathBuf,
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        cargo_home: PathBuf,
        #[arg(long)]
        v8_cache: PathBuf,
    },
    /// Resolve and verify the exact source-pinned V8 archive and binding through Agh.
    #[cfg(unix)]
    V8 {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        intent: PathBuf,
        #[arg(long)]
        cache: PathBuf,
    },
    /// Run a release stage with immutable inputs and explicit writable output roots.
    #[cfg(unix)]
    Protect {
        #[command(flatten)]
        roots: ProtectedArgs,
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
    /// Run a protected compiler stage under atpkg's exact source-selected lease.
    #[cfg(unix)]
    Compile {
        #[command(flatten)]
        roots: ProtectedArgs,
        #[arg(long)]
        intent: PathBuf,
        #[arg(long)]
        publisher: PathBuf,
        #[arg(last = true, required = true)]
        arguments: Vec<std::ffi::OsString>,
    },
    #[cfg(unix)]
    #[command(hide = true)]
    CompileWorker {
        #[command(flatten)]
        roots: ProtectedArgs,
        #[arg(long)]
        intent: PathBuf,
        #[arg(long)]
        expected_intent: String,
        #[arg(long)]
        receipt: PathBuf,
        #[arg(long)]
        nonce: String,
        #[arg(long)]
        stage_plan: Option<PathBuf>,
        #[arg(last = true, required = true)]
        arguments: Vec<std::ffi::OsString>,
    },
    /// Freeze regular version, exact source, target and compiler inputs. Does not publish.
    Prepare {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long, default_value = "aarch64-apple-darwin")]
        target: String,
        #[arg(long)]
        intent: PathBuf,
    },
    /// Read the frozen intent. This establishes no build, publication or installation.
    Status {
        #[arg(long)]
        intent: PathBuf,
    },
}

fn main() -> Result<()> {
    let result = run();
    #[cfg(unix)]
    if let Err(error) = &result
        && let Some(code) = stage_status::exit_code(error.as_ref())
    {
        eprintln!("{error}");
        std::process::exit(code);
    }
    result
}

fn run() -> Result<()> {
    let args = Args::parse();
    #[cfg(unix)]
    {
        if matches!(
            args.command,
            Action::Build { .. }
                | Action::Compile { .. }
                | Action::Ripgrep { .. }
                | Action::Publish(_)
                | Action::Cycle(_)
                | Action::CheckPredecessor(_)
        ) {
            return outer::run(args.release_owner_fd, || dispatch(args.command));
        }
        if args.release_owner_fd.is_some() {
            return Err("unexpected release owner descriptor for this action".into());
        }
    }
    dispatch(args.command)
}

fn dispatch(action: Action) -> Result<()> {
    let release = match action {
        #[cfg(unix)]
        Action::Publish(args) => return publish(args, None),
        #[cfg(unix)]
        Action::Cycle(args) => {
            return codex_alab_release::schedule::cycle(
                &args.repo,
                &args.publisher,
                &args.state,
                |intent| {
                    build(
                        args.repo.clone(),
                        intent.to_owned(),
                        args.publisher.clone(),
                        args.state.clone(),
                        args.cargo_home.clone(),
                        args.v8_cache.clone(),
                    )
                },
                |intent, product| {
                    ripgrep(
                        args.repo.clone(),
                        intent.to_owned(),
                        product.to_owned(),
                        args.publisher.clone(),
                        args.state.clone(),
                        args.cargo_home.clone(),
                    )
                },
                outer::check,
                |intent, build, ready| {
                    outer::check()?;
                    publish(
                        PublishArgs {
                            repo: args.repo.clone(),
                            publisher: args.publisher.clone(),
                            state: args.state.clone(),
                            cargo_home: args.cargo_home.clone(),
                            v8_cache: args.v8_cache.clone(),
                            intent,
                            build,
                        },
                        Some(ready),
                    )
                },
            );
        }
        #[cfg(unix)]
        Action::CheckPredecessor(args) => {
            println!("{}", serde_json::to_string(&predecessor::run(args)?)?);
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        Action::VerifySandbox => {
            codex_alab_release::protected::verify_native_wrapper()?;
            codex_alab_release::protected::verify_native_policy()?;
            println!("Native sandbox controls: 2 passed");
            return Ok(());
        }
        #[cfg(unix)]
        Action::Ripgrep {
            repo,
            intent,
            observation,
            publisher,
            state,
            cargo_home,
        } => {
            return ripgrep(repo, intent, observation, publisher, state, cargo_home).map(|_| ());
        }
        #[cfg(unix)]
        Action::Assemble {
            repo,
            intent,
            observation,
            ripgrep_observation,
            state,
            out,
        } => {
            let release = load_intent(&intent, &repo)?;
            return codex_alab_release::assembly::assemble(
                &repo,
                &release,
                &observation,
                &ripgrep_observation,
                &state,
                &out,
            );
        }
        #[cfg(unix)]
        Action::Build {
            repo,
            intent,
            publisher,
            state,
            cargo_home,
            v8_cache,
        } => {
            return build(repo, intent, publisher, state, cargo_home, v8_cache).map(|_| ());
        }
        #[cfg(unix)]
        Action::V8 {
            repo,
            intent,
            cache,
        } => {
            let intent = load_intent(&intent, &repo)?;
            let artifacts = codex_alab_release::v8::resolve(&repo, &intent, &cache)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "version": artifacts.version, "archive": artifacts.archive,
                    "binding": artifacts.binding, "intent": intent.id()?,
                    "publication": "unverified",
                }))?
            );
            return Ok(());
        }
        #[cfg(unix)]
        Action::Protect { roots, command } => {
            let (program, arguments) = command
                .split_first()
                .ok_or("a protected command is required")?;
            let protected = codex_alab_release::protected::Protected::new(
                &roots.repo,
                &roots.cargo_home,
                &roots.read,
                &roots.write,
            )?;
            eprintln!("protected scratch: {}", protected.scratch_path().display());
            let status = protected.run(program, arguments)?;
            return if status.success() {
                Ok(())
            } else {
                Err(format!("protected command failed: {status}").into())
            };
        }
        #[cfg(unix)]
        Action::Compile {
            mut roots,
            intent,
            publisher,
            arguments,
        } => {
            space::public_compile(
                &roots.repo,
                &mut roots.cargo_home,
                &mut roots.write,
                &arguments,
            )?;
            return compile(roots, intent, publisher, arguments, None).map(|_| ());
        }
        #[cfg(unix)]
        Action::CompileWorker {
            roots,
            intent,
            expected_intent,
            receipt,
            nonce,
            stage_plan,
            arguments,
        } => {
            return compile_worker(
                roots,
                intent,
                &expected_intent,
                receipt,
                nonce,
                arguments,
                stage_plan,
            );
        }
        Action::Prepare {
            repo,
            source,
            target,
            intent,
        } => {
            let release = Release::prepare(&repo, &source, &target)?;
            release.save(&intent)?;
            release
        }
        Action::Status { intent } => Release::load(&intent)?,
    };
    println!("{}", serde_json::to_string_pretty(&release)?);
    println!("Intent: {}\nPublication: unverified", release.id()?);
    Ok(())
}

#[cfg(unix)]
fn load_intent(path: &std::path::Path, repo: &std::path::Path) -> Result<Release> {
    let intent = Release::load(path)?;
    intent.check_local(repo)?;
    Ok(intent)
}

#[cfg(unix)]
fn compile(
    roots: ProtectedArgs,
    intent: PathBuf,
    publisher: PathBuf,
    arguments: Vec<std::ffi::OsString>,
    stage: Option<CompilerStage>,
) -> Result<Completion> {
    let release = load_intent(&intent, &roots.repo)?;
    let intent = std::fs::canonicalize(intent)?;
    if Release::prepare(&roots.repo, &release.source, &release.target)? != release {
        return Err("release source changed before compiler admission".into());
    }
    let control = codex_alab_release::protected::short_scratch("release-control.")?;
    let control = std::fs::canonicalize(control.keep())?;
    let nonce = control
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid native stage nonce")?
        .to_owned();
    let receipt = control.join("completion.json");
    eprintln!("native compiler control: {}", control.display());
    let expected = Completion {
        schema: 3,
        intent: release.id()?,
        nonce: nonce.clone(),
        arguments: arguments_id(&arguments),
        product: None,
        predecessor: None,
    };
    let mut command = std::process::Command::new(std::fs::canonicalize(publisher)?);
    command
        .args(["toolchain", "run", "--repo"])
        .arg(std::fs::canonicalize(&roots.repo)?)
        .args(["--source", &release.source, "--"])
        .arg(std::env::current_exe()?)
        .arg("compile-worker")
        .arg("--repo")
        .arg(std::fs::canonicalize(&roots.repo)?)
        .arg("--cargo-home")
        .arg(std::fs::canonicalize(roots.cargo_home)?)
        .arg("--intent")
        .arg(&intent)
        .arg("--expected-intent")
        .arg(release.id()?)
        .arg("--receipt")
        .arg(&receipt)
        .arg("--nonce")
        .arg(nonce);
    let plan_path = control.join("stage.json");
    if let Some(plan) = &stage {
        codex_alab_release::save_record(&plan_path, &serde_json::to_vec(plan)?)?;
        command.arg("--stage-plan").arg(&plan_path);
    }
    for path in roots.read {
        command.arg("--read").arg(std::fs::canonicalize(path)?);
    }
    for path in roots.write {
        command.arg("--write").arg(std::fs::canonicalize(path)?);
    }
    if matches!(stage, Some(CompilerStage::Predecessor { .. })) {
        command.stdout(std::process::Stdio::from(std::io::stderr()));
    }
    let status = outer::publisher_status(command.arg("--").args(arguments))?;
    if load_intent(&intent, &roots.repo)? != release {
        return Err("release intent changed during its native compiler stage".into());
    }
    outer::check()?;
    stage_status::require_success("native compiler controller", status)?;
    let bytes = codex_alab_release::record_bytes(&receipt)
        .map_err(|error| format!("native compiler produced no valid completion record: {error}"))?;
    let mut observed: Completion = serde_json::from_slice(&bytes)?;
    let product = observed.product.take();
    let predecessor = observed.predecessor.take();
    if observed != expected
        || product.is_some() != matches!(stage, Some(CompilerStage::Product { .. }))
        || predecessor.is_some() != matches!(stage, Some(CompilerStage::Predecessor { .. }))
    {
        return Err("native compiler completion does not match the requested stage".into());
    }
    // The qualified native supervisor has now drained its controller group.
    // Reclaim only this parent's private control record, never a path supplied
    // by a child receipt. Worker scratch remains separately retained.
    std::fs::remove_file(&receipt)?;
    if stage.is_some() {
        std::fs::remove_file(&plan_path)?;
    }
    std::fs::remove_dir(&control)?;
    observed.product = product;
    observed.predecessor = predecessor;
    Ok(observed)
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    schema: u32,
    intent: String,
    nonce: String,
    arguments: String,
    product: Option<codex_alab_release::product::Build>,
    predecessor: Option<predecessor::Observation>,
}

#[cfg(unix)]
fn arguments_id(arguments: &[std::ffi::OsString]) -> String {
    use sha2::Digest;
    use std::os::unix::ffi::OsStrExt;
    let mut hash = sha2::Sha256::new();
    for argument in arguments {
        let bytes = argument.as_bytes();
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}

#[cfg(unix)]
fn compile_worker(
    mut roots: ProtectedArgs,
    intent: PathBuf,
    expected: &str,
    receipt: PathBuf,
    nonce: String,
    arguments: Vec<std::ffi::OsString>,
    stage_plan: Option<PathBuf>,
) -> Result<()> {
    let worker = codex_alab_release::protected::Worker::new()?;
    let release = load_intent(&intent, &roots.repo)?;
    if release.id()? != expected {
        return Err("the compiler stage received another release intent".into());
    }
    let selected = |key: &str| -> Result<PathBuf> {
        let path =
            PathBuf::from(std::env::var_os(key).ok_or("no native project compiler selection")?);
        if !path.is_absolute() || !path.is_file() {
            return Err(format!("invalid native project tool {key}").into());
        }
        Ok(path)
    };
    let front = selected("ATPKG_PROJECT_FRONT")?;
    let targo = selected("ATPKG_PROJECT_TARGO")?;
    let stage: Option<CompilerStage> = stage_plan
        .as_ref()
        .map(|path| -> Result<_> {
            Ok(serde_json::from_slice(&codex_alab_release::record_bytes(
                path,
            )?)?)
        })
        .transpose()?;
    let inputs = stage.as_ref().map(CompilerStage::inputs);
    if let Some(plan) = inputs {
        plan.check(&roots.repo, &release)?;
        let expected = match &stage {
            Some(CompilerStage::Predecessor { candidate, .. }) => {
                candidate.check()?;
                roots.read.push(candidate.executable.clone());
                predecessor::arguments(plan)
            }
            Some(CompilerStage::Product {
                upstream, report, ..
            }) => {
                if upstream != &codex_alab_release::product::upstream(&roots.repo, &release)? {
                    return Err("frozen upstream product identity changed".into());
                }
                codex_alab_release::product::Certification::check_fresh(&plan.target, report)?;
                codex_alab_release::product::certified_arguments(&release.source, report)
            }
            None => return Err("missing compiler stage".into()),
        };
        if arguments != expected {
            return Err("canonical compiler arguments changed".into());
        }
        roots.read.push(plan.source.clone());
        roots.read.push(plan.v8_archive.clone());
        roots.read.push(plan.v8_binding.clone());
    }
    roots.read.push(intent.clone());
    roots.read.push(
        receipt
            .parent()
            .ok_or("native completion has no control directory")?
            .to_path_buf(),
    );
    let cwd = inputs
        .map(|plan| plan.source.join("codex-rs"))
        .unwrap_or_else(|| roots.repo.clone());
    let protected = codex_alab_release::protected::Protected::new(
        &cwd,
        &roots.cargo_home,
        &roots.read,
        &roots.write,
    )?;
    eprintln!("protected scratch: {}", protected.scratch_path().display());
    let mut compiler = if let Some(CompilerStage::Product { inputs, .. }) = &stage {
        let driver = inputs.source.join("alab/certified-build.sh");
        let mut command = worker.command(&protected, driver.as_os_str(), &arguments, true)?;
        command.env("CARGO_TARGET_DIR", &inputs.target);
        command
    } else {
        let mut compiler_args = vec!["__build_admit".into(), targo.into_os_string()];
        compiler_args.extend(arguments.clone());
        worker.command(&protected, front.as_os_str(), &compiler_args, false)?
    };
    compiler
        .env("PCRE2_SYS_STATIC", "1")
        .env("STABLE_GIT_COMMIT", &release.source)
        .env("CODEX_ALAB_PRODUCT_VERSION", &release.version)
        .env("CODEX_ALAB_BUILD_VERSION", &release.client_version)
        .env("CODEX_ALAB_UPSTREAM_VERSION", &release.baseline);
    if let Some(plan) = inputs {
        compiler
            .env("RUSTY_V8_ARCHIVE", &plan.v8_archive)
            .env("RUSTY_V8_SRC_BINDING_PATH", &plan.v8_binding)
            .env("CARGO_INCREMENTAL", "0");
    }
    if let Some(CompilerStage::Product { upstream, .. }) = &stage {
        compiler.env("CODEX_ALAB_UPSTREAM_COMMIT", &upstream.commit);
    }
    let test_binary = if let Some(CompilerStage::Predecessor { inputs, .. }) = &stage {
        predecessor::compile_test(&worker, &mut compiler, inputs).map(Some)
    } else {
        let status = compiler.status()?;
        stage_status::require_success("protected compiler command", status).map(|()| None)
    };
    if load_intent(&intent, &roots.repo)? != release {
        return Err("compiler stage changed its release intent".into());
    }
    worker.check()?;
    let test_binary = test_binary?;
    let compiler = if let Some(plan) = inputs {
        plan.check(&roots.repo, &release)?;
        let trustc = selected("ATPKG_PROJECT_TRUSTC")?;
        let query = |args: &[std::ffi::OsString]| -> Result<String> {
            let output = worker
                .disposable_command(&protected, trustc.as_os_str(), args)?
                .output()?;
            worker.check()?;
            stage_status::require_success("selected compiler identity query", output.status)?;
            Ok(String::from_utf8(output.stdout)?)
        };
        let vv = query(&["-vV".into()])?;
        if !vv
            .lines()
            .any(|line| line == format!("host: {}", release.target))
        {
            return Err("canonical product build is not on its native target".into());
        }
        let sysroot = query(&["--print".into(), "sysroot".into()])?;
        let execution = std::fs::canonicalize(
            std::env::var_os("ATPKG_PROJECT_EXECUTION")
                .ok_or("no selected compiler execution root")?,
        )?;
        if std::fs::canonicalize(sysroot.trim())? != execution {
            return Err("compiler sysroot differs from native selection".into());
        }
        Some(codex_alab_release::product::observe_compiler(
            &plan.target,
            &vv,
            sysroot.trim(),
        )?)
    } else {
        None
    };
    let mut completion = Completion {
        schema: 3,
        intent: release.id()?,
        nonce,
        arguments: arguments_id(&arguments),
        product: None,
        predecessor: None,
    };
    match (stage, compiler) {
        (
            Some(CompilerStage::Product {
                inputs: plan,
                upstream,
                report,
            }),
            Some(compiler),
        ) => {
            if upstream != codex_alab_release::product::upstream(&roots.repo, &release)? {
                return Err("upstream product identity changed during the build".into());
            }
            completion.product = Some(codex_alab_release::product::Build {
                intent: release.id()?,
                source: release.source.clone(),
                version: release.version.clone(),
                target: plan.target.clone(),
                compiler,
                certification: codex_alab_release::product::Certification::observe(
                    &plan.target,
                    &report,
                )?,
                binaries: codex_alab_release::product::binaries(&plan.target)?,
                upstream,
            });
        }
        (Some(CompilerStage::Predecessor { inputs, candidate }), Some(compiler)) => {
            let binary = test_binary.ok_or("missing predecessor test binary")?;
            // The test only needs its two executables, scratch and no mutable compiler outputs.
            let empty_home = tempfile::tempdir_in(protected.scratch_path())?;
            let runtime = codex_alab_release::protected::Protected::new(
                &cwd,
                empty_home.path(),
                &[binary.clone(), candidate.executable.clone()],
                &[],
            )?;
            completion.predecessor = Some(predecessor::observe(
                &worker, &runtime, &release, &candidate, &binary, compiler,
            )?);
            inputs.check(&roots.repo, &release)?;
        }
        (None, None) => {}
        _ => return Err("missing stage compiler observation".into()),
    }
    use std::io::Write;
    worker.check()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&receipt)?;
    file.write_all(&serde_json::to_vec(&completion)?)?;
    file.sync_all()?;
    std::fs::File::open(receipt.parent().ok_or("missing completion directory")?)?.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn build(
    repo: PathBuf,
    intent: PathBuf,
    publisher: PathBuf,
    state: PathBuf,
    cargo_home: PathBuf,
    v8_cache: PathBuf,
) -> Result<PathBuf> {
    let release = load_intent(&intent, &repo)?;
    space::product(&repo, &release, &state, &cargo_home, &v8_cache)?;
    if Release::prepare(&repo, &release.source, &release.target)? != release {
        return Err("product source changed before checkout".into());
    }
    let checkout = codex_alab_release::source::Checkout::prepare(&repo, &release, &state)?;
    let upstream = codex_alab_release::product::fetch_upstream(&repo, &release)?;
    let v8 = codex_alab_release::v8::resolve(&repo, &release, &v8_cache)?;
    let plan = codex_alab_release::product::Inputs::new(&release, &checkout, &v8)?;
    space::product(&repo, &release, &state, &cargo_home, &v8_cache)?;
    let report = codex_alab_release::product::Certification::fresh(&plan.target)?;
    let arguments = codex_alab_release::product::certified_arguments(&release.source, &report);
    let roots = ProtectedArgs {
        repo,
        cargo_home,
        read: vec![],
        write: vec![plan.target.clone()],
    };
    let completed = compile(
        roots,
        intent,
        publisher,
        arguments,
        Some(CompilerStage::Product {
            report,
            inputs: plan,
            upstream,
        }),
    )?;
    checkout.check(&release)?;
    v8.check()?;
    let record = completed
        .product
        .ok_or("canonical build produced no product observation")?;
    record.check(&release)?;
    let bytes = serde_json::to_vec_pretty(&record)?;
    use sha2::Digest;
    let id = format!("{:x}", sha2::Sha256::digest(&bytes));
    let path = checkout.campaign.join(format!("build-{id}.json"));
    codex_alab_release::save_record(&path, &bytes)?;
    println!(
        "Build observation: {}\nQualification and publication: unverified",
        path.display()
    );
    Ok(path)
}

#[cfg(unix)]
fn ripgrep(
    repo: PathBuf,
    intent: PathBuf,
    observation: PathBuf,
    publisher: PathBuf,
    state: PathBuf,
    cargo_home: PathBuf,
) -> Result<PathBuf> {
    use codex_alab_release::product;
    use codex_alab_release::protected::Protected;
    use codex_alab_release::ripgrep;
    use codex_alab_release::source::Checkout;
    use sha2::Digest;
    let release = load_intent(&intent, &repo)?;
    space::ripgrep(&repo, &release, &state, &cargo_home)?;
    let checkout = Checkout::reopen(&repo, &release, &state)?;
    let product = product::Build::from_observation(
        &codex_alab_release::record_bytes(&observation)?,
        &repo,
        &release,
        &checkout,
    )?;
    let version = codex_alab_release::assembly::ripgrep_version(&repo, &release)?;
    // Preserve failed attempts. Never reuse an unobserved executable or Cargo install's
    // already-installed shortcut as evidence that this operation compiled anything.
    let attempt = tempfile::Builder::new()
        .prefix("ripgrep-")
        .tempdir_in(&checkout.campaign)?
        .keep();
    let target = attempt.join("target.noindex");
    let root = attempt.join("install.noindex");
    std::fs::create_dir(&target)?;
    std::fs::create_dir(&root)?;
    let args = ripgrep::arguments(&version, &target, &root);
    compile(
        ProtectedArgs {
            repo: repo.clone(),
            cargo_home: cargo_home.clone(),
            read: vec![],
            write: vec![target.clone(), root.clone()],
        },
        intent,
        publisher,
        args,
        None,
    )?;
    let compiler = product::observe_compiler(
        &target,
        &product.compiler.rustc_vv,
        &product.compiler.sysroot,
    )?;
    if compiler != product.compiler {
        return Err("ripgrep was built with another compiler".into());
    }
    let binary = root.join("bin/rg");
    let protected = Protected::new(&repo, &cargo_home, std::slice::from_ref(&root), &[])?;
    let output = protected
        .command(binary.as_os_str(), &["--version".into()])
        .output()?;
    ripgrep::check_version(&output, &version)?;
    let build = ripgrep::Build {
        intent: release.id()?,
        version,
        sha256: product::file_hash(&binary)?,
        binary,
        target,
        compiler,
    };
    checkout.check(&release)?;
    product.check(&release)?;
    build.check(&repo, &release, &product.compiler, &checkout.campaign)?;
    let bytes = serde_json::to_vec_pretty(&build)?;
    let id = format!("{:x}", sha2::Sha256::digest(&bytes));
    let path = checkout.campaign.join(format!("ripgrep-{id}.json"));
    codex_alab_release::save_record(&path, &bytes)?;
    println!(
        "Ripgrep observation: {}\nQualification and publication: unverified",
        path.display()
    );
    Ok(path)
}

#[cfg(unix)]
fn publish(args: PublishArgs, ready: Option<&codex_alab_release::schedule::Ready>) -> Result<()> {
    let release = load_intent(&args.intent, &args.repo)?;
    let campaign = codex_alab_release::workflow::Campaign::open_with_source_admission(
        &args.repo,
        &release,
        &args.state,
        args.build,
        &args.publisher,
        || {
            space::product(
                &args.repo,
                &release,
                &args.state,
                &args.cargo_home,
                &args.v8_cache,
            )
        },
    )?;
    if let Some(ready) = ready {
        ready.check()?;
        campaign.retain_stage("product", &ready.product)?;
        campaign.retain_stage("ripgrep", &ready.ripgrep)?;
    }
    let product = match campaign.stage("product")? {
        Some(record) => record,
        None => {
            let record = build(
                args.repo.clone(),
                args.intent.clone(),
                args.publisher.clone(),
                args.state.clone(),
                args.cargo_home.clone(),
                args.v8_cache.clone(),
            )?;
            campaign.retain_stage("product", &record)?;
            record
        }
    };
    outer::check()?;
    let rg = match campaign.stage("ripgrep")? {
        Some(record) => record,
        None => {
            let record = ripgrep(
                args.repo.clone(),
                args.intent.clone(),
                product.clone(),
                args.publisher.clone(),
                args.state.clone(),
                args.cargo_home.clone(),
            )?;
            campaign.retain_stage("ripgrep", &record)?;
            record
        }
    };
    campaign.check()?;
    outer::check()?;
    codex_alab_release::assembly::assemble(
        &args.repo,
        &release,
        &product,
        &rg,
        &args.state,
        &campaign.package(),
    )?;
    campaign.publish(
        &args.repo,
        &release,
        || {
            if ready.is_some() {
                codex_alab_release::schedule::check_dispatch(
                    &args.repo,
                    &args.publisher,
                    &release,
                    args.build,
                )?;
            }
            outer::check()
        },
        |previous, successor, executable| {
            let observation = predecessor::run(predecessor::Args {
                repo: args.repo.clone(),
                publisher: args.publisher.clone(),
                state: args.state.clone(),
                cargo_home: args.cargo_home.clone(),
                v8_cache: args.v8_cache.clone(),
                source: previous.source.clone(),
                target: release.target.clone(),
                candidate: executable.to_owned(),
                candidate_version: successor.version.clone(),
                candidate_sha256: successor.executable_sha256.clone(),
            })?;
            if observation.predecessor_version != previous.version {
                return Err(
                    "the frozen predecessor source does not produce the signed product version"
                        .into(),
                );
            }
            Ok(())
        },
    )?;
    println!(
        "Native package publisher completed; installation and running-process adoption remain unverified."
    );
    Ok(())
}
