// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use clap::Subcommand;
use codex_alab_release::Release;
use codex_alab_release::Result;
use std::path::PathBuf;

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

#[derive(Subcommand)]
enum Action {
    /// Build, assemble and publish one frozen regular release through native atpkg.
    #[cfg(unix)]
    Publish(PublishArgs),
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
        product_plan: Option<PathBuf>,
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
    let release = match Args::parse().command {
        #[cfg(unix)]
        Action::Publish(args) => return publish(args),
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
            roots,
            intent,
            publisher,
            arguments,
        } => {
            return compile(roots, intent, publisher, arguments, None).map(|_| ());
        }
        #[cfg(unix)]
        Action::CompileWorker {
            roots,
            intent,
            expected_intent,
            receipt,
            nonce,
            product_plan,
            arguments,
        } => {
            return compile_worker(
                roots,
                intent,
                &expected_intent,
                receipt,
                nonce,
                arguments,
                product_plan,
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
    product: Option<codex_alab_release::product::Inputs>,
) -> Result<Completion> {
    let release = load_intent(&intent, &roots.repo)?;
    let intent = std::fs::canonicalize(intent)?;
    if Release::prepare(&roots.repo, &release.source, &release.target)? != release {
        return Err("release source changed before compiler admission".into());
    }
    std::fs::create_dir_all("/tmp/.ubd")?;
    let control = tempfile::Builder::new()
        .prefix("release-control.")
        .tempdir_in("/tmp/.ubd")?;
    let control = std::fs::canonicalize(control.keep())?;
    let nonce = control
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid native stage nonce")?
        .to_owned();
    let receipt = control.join("completion.json");
    eprintln!("native compiler control: {}", control.display());
    let expected = Completion {
        schema: 2,
        intent: release.id()?,
        nonce: nonce.clone(),
        arguments: arguments_id(&arguments),
        product: None,
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
    let product_mode = product.is_some();
    let plan_path = control.join("product.json");
    if let Some(plan) = &product {
        codex_alab_release::save_record(&plan_path, &serde_json::to_vec(plan)?)?;
        command.arg("--product-plan").arg(&plan_path);
    }
    for path in roots.read {
        command.arg("--read").arg(std::fs::canonicalize(path)?);
    }
    for path in roots.write {
        command.arg("--write").arg(std::fs::canonicalize(path)?);
    }
    let status = command.arg("--").args(arguments).status()?;
    if load_intent(&intent, &roots.repo)? != release {
        return Err("release intent changed during its native compiler stage".into());
    }
    if !status.success() {
        return Err(format!("native compiler controller failed: {status}").into());
    }
    let bytes = codex_alab_release::record_bytes(&receipt)
        .map_err(|error| format!("native compiler produced no valid completion record: {error}"))?;
    let mut observed: Completion = serde_json::from_slice(&bytes)?;
    let product = observed.product.take();
    if observed != expected || product.is_some() != product_mode {
        return Err("native compiler completion does not match the requested stage".into());
    }
    // The qualified native supervisor has now drained its controller group.
    // Reclaim only this parent's private control record, never a path supplied
    // by a child receipt. Worker scratch remains separately retained.
    std::fs::remove_file(&receipt)?;
    if product_mode {
        std::fs::remove_file(&plan_path)?;
    }
    std::fs::remove_dir(&control)?;
    observed.product = product;
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
    product_plan: Option<PathBuf>,
) -> Result<()> {
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
    let product: Option<codex_alab_release::product::Inputs> = product_plan
        .as_ref()
        .map(|path| -> Result<_> {
            Ok(serde_json::from_slice(&codex_alab_release::record_bytes(
                path,
            )?)?)
        })
        .transpose()?;
    if let Some(plan) = &product {
        plan.check(&roots.repo, &release)?;
        if arguments != plan.arguments() {
            return Err("canonical product compiler arguments changed".into());
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
    let cwd = product
        .as_ref()
        .map(|plan| plan.source.join("codex-rs"))
        .unwrap_or_else(|| roots.repo.clone());
    let protected = codex_alab_release::protected::Protected::new(
        &cwd,
        &roots.cargo_home,
        &roots.read,
        &roots.write,
    )?;
    eprintln!("protected scratch: {}", protected.scratch_path().display());
    let mut compiler_args = vec!["__build_admit".into(), targo.into_os_string()];
    compiler_args.extend(arguments.clone());
    let mut compiler = protected.command(front.as_os_str(), &compiler_args);
    compiler
        .env("PCRE2_SYS_STATIC", "1")
        .env("STABLE_GIT_COMMIT", &release.source)
        .env("CODEX_ALAB_PRODUCT_VERSION", &release.version)
        .env("CODEX_ALAB_BUILD_VERSION", &release.client_version)
        .env("CODEX_ALAB_UPSTREAM_VERSION", &release.baseline);
    if let Some(plan) = &product {
        compiler
            .env("RUSTY_V8_ARCHIVE", &plan.v8_archive)
            .env("RUSTY_V8_SRC_BINDING_PATH", &plan.v8_binding)
            .env("CODEX_ALAB_UPSTREAM_COMMIT", &plan.upstream.commit)
            .env("CARGO_INCREMENTAL", "0");
    }
    let status = compiler.status()?;
    if load_intent(&intent, &roots.repo)? != release {
        return Err("compiler stage changed its release intent".into());
    }
    if !status.success() {
        return Err(format!("protected compiler command failed: {status}").into());
    }
    let build = if let Some(plan) = &product {
        plan.check(&roots.repo, &release)?;
        let trustc = selected("ATPKG_PROJECT_TRUSTC")?;
        let query = |args: &[std::ffi::OsString]| -> Result<String> {
            let output = protected.command(trustc.as_os_str(), args).output()?;
            if !output.status.success() {
                return Err("could not observe selected compiler".into());
            }
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
        let compiler =
            codex_alab_release::product::observe_compiler(&plan.target, &vv, sysroot.trim())?;
        Some(codex_alab_release::product::Build {
            intent: release.id()?,
            source: release.source.clone(),
            version: release.version.clone(),
            target: plan.target.clone(),
            compiler,
            binaries: codex_alab_release::product::binaries(&plan.target)?,
            upstream: plan.upstream.clone(),
        })
    } else {
        None
    };
    let completion = Completion {
        schema: 2,
        intent: release.id()?,
        nonce,
        arguments: arguments_id(&arguments),
        product: build,
    };
    use std::io::Write;
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
    if Release::prepare(&repo, &release.source, &release.target)? != release {
        return Err("product source changed before checkout".into());
    }
    let checkout = codex_alab_release::source::Checkout::prepare(&repo, &release, &state)?;
    let v8 = codex_alab_release::v8::resolve(&repo, &release, &v8_cache)?;
    let target = checkout.output()?;
    let (archive_hash, binding_hash) = v8.hashes();
    let plan = codex_alab_release::product::Inputs {
        intent: release.id()?,
        campaign: checkout.campaign.clone(),
        source: checkout.root.clone(),
        target: target.clone(),
        v8_archive: v8.archive.clone(),
        v8_binding: v8.binding.clone(),
        v8_archive_sha256: archive_hash.to_owned(),
        v8_binding_sha256: binding_hash.to_owned(),
        upstream: codex_alab_release::product::upstream(&repo, &release)?,
    };
    let arguments = plan.arguments();
    let roots = ProtectedArgs {
        repo,
        cargo_home,
        read: vec![],
        write: vec![target],
    };
    let completed = compile(roots, intent, publisher, arguments, Some(plan))?;
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
    let checkout = Checkout::prepare(&repo, &release, &state)?;
    let product: product::Build =
        serde_json::from_slice(&codex_alab_release::record_bytes(&observation)?)?;
    if product.target != checkout.output()? {
        return Err("ripgrep needs this campaign's product observation".into());
    }
    product.check(&release)?;
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
    build.check(&repo, &release, &product.compiler)?;
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
fn publish(args: PublishArgs) -> Result<()> {
    let release = load_intent(&args.intent, &args.repo)?;
    let campaign = codex_alab_release::workflow::Campaign::open(
        &args.repo,
        &release,
        &args.state,
        args.build,
        &args.publisher,
    )?;
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
    codex_alab_release::assembly::assemble(
        &args.repo,
        &release,
        &product,
        &rg,
        &args.state,
        &campaign.package(),
    )?;
    campaign.publish(&args.repo, &release)?;
    println!(
        "Native package publisher completed; installation and running-process adoption remain unverified."
    );
    Ok(())
}
