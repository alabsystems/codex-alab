// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The launch snapshot's `argv[0]` resolution.

use crate::life;
use pretty_assertions::assert_eq;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

// ---- argv[0] through the launch PATH

fn executable(dir: &Path, name: &str, mode: u32) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, "#!/bin/sh\n").unwrap_or_else(|err| panic!("{err}"));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|err| panic!("{err}"));
    path
}

#[test]
fn argv0_resolves_through_the_launch_path() {
    let root = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let first = root.path().join("first");
    let second = root.path().join("second");
    for dir in [&first, &second] {
        std::fs::create_dir(dir).unwrap_or_else(|err| panic!("{err}"));
    }
    executable(&first, "codex", 0o644);
    let twin = executable(&second, "codex", 0o755);
    let path = std::env::join_paths([&first, &second]).unwrap_or_else(|err| panic!("{err}"));
    let cwd = Some(Path::new("/launch/cwd"));
    assert_eq!(
        life::resolve_argv0("codex".as_ref(), Some(&path), cwd),
        Some(twin)
    );
    assert_eq!(
        life::resolve_argv0("./bin/codex".as_ref(), Some(&path), cwd),
        Some(PathBuf::from("/launch/cwd/./bin/codex"))
    );
    assert_eq!(
        life::resolve_argv0("/abs/codex".as_ref(), None, None),
        Some(PathBuf::from("/abs/codex"))
    );
    assert_eq!(
        life::resolve_argv0("missing".as_ref(), Some(&path), cwd),
        None
    );

    // A raw fallback retains an explicit managed route even while that route is
    // absent. Publication can select B before A reaches capture_launch.
    let prefix = root.path().join("pkg");
    let entry = prefix.join("store/codex/exec/codex");
    let a = alab_package(&prefix.join("store/codex/101"));
    let b = alab_package(&prefix.join("store/codex/102"));
    let primary = prefix.join("bin/codex");
    managed_primary(&primary, &entry, &a);
    let vars = fallback_vars(42, &entry, &a);
    assert_eq!(
        life::managed_launch_path(&vars, 42, &a),
        Some(entry.clone())
    );
    managed_primary(&primary, &entry, &b);
    assert_eq!(
        life::managed_launch_path(&vars, 42, &a),
        Some(entry.clone())
    );
    // A child inherits the variables, not the exporting process's identity. No hint
    // means an intentional physical launch, even for an ALab-looking package.
    assert_eq!(life::managed_launch_path(&vars, 43, &a), None);
    assert_eq!(life::managed_launch_path(&[], 42, &a), None);
    let mut duplicate = vars.clone();
    duplicate.push(vars[0].clone());
    assert_eq!(life::managed_launch_path(&duplicate, 42, &a), None);
    assert_eq!(
        life::managed_launch_path(&fallback_vars(42, Path::new("relative"), &a), 42, &a),
        None
    );
    assert_eq!(
        life::managed_launch_path(&fallback_vars(42, &entry, &b), 42, &a),
        None
    );
    let foreign = alab_package(&root.path().join("foreign/store/codex/101"));
    assert_eq!(
        life::managed_launch_path(&fallback_vars(42, &entry, &foreign), 42, &foreign),
        None
    );
    let leading_zero = alab_package(&prefix.join("store/codex/0101"));
    assert_eq!(
        life::managed_launch_path(&fallback_vars(42, &entry, &leading_zero), 42, &leading_zero),
        None
    );

    // A dev-link's old A package remains the physical proof after its marker and
    // primary move to B. Neither mutable selection must still name A.
    let dev_a = alab_package(&root.path().join("dev-a"));
    let dev_b = alab_package(&root.path().join("dev-b"));
    std::fs::create_dir_all(prefix.join("links")).unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(
        prefix.join("links/codex"),
        format!(
            "schema = 1\nprogram = \"codex\"\ncheckout = {:?}\nbins = [\"bin/codex\"]\n",
            package_root(&dev_a)
                .to_str()
                .unwrap_or_else(|| panic!("a UTF-8 fixture path"))
        ),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    let dev_vars = fallback_vars(42, &entry, &dev_a);
    std::fs::write(
        prefix.join("links/codex"),
        format!(
            "schema = 1\nprogram = \"codex\"\ncheckout = {:?}\nbins = [\"bin/codex\"]\n",
            package_root(&dev_b)
                .to_str()
                .unwrap_or_else(|| panic!("a UTF-8 fixture path"))
        ),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    managed_primary(&primary, &entry, &dev_b);
    assert_eq!(
        life::managed_launch_path(&dev_vars, 42, &dev_a),
        Some(entry.clone())
    );
    std::fs::remove_file(package_root(&dev_a).join("alab-build.json"))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(life::managed_launch_path(&dev_vars, 42, &dev_a), None);
    std::fs::write(package_root(&dev_a).join("alab-build.json"), "{}")
        .unwrap_or_else(|err| panic!("{err}"));
    std::os::unix::fs::symlink(
        package_root(&dev_a).join("bin"),
        package_root(&dev_a).join("alias-dir"),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(
        package_root(&dev_a).join("codex-package.json"),
        r#"{"layoutVersion":1,"entrypoint":"alias-dir/codex","version":"A"}"#,
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(life::managed_launch_path(&dev_vars, 42, &dev_a), None);
    // Vendor/unmanaged primaries cannot establish a managed logical route.
    std::fs::write(
        &primary,
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", b.display()),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(life::managed_launch_path(&vars, 42, &a), None);
    managed_primary(&primary, &entry, &b);
    let primary_bytes = std::fs::read(&primary).unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(
        &primary,
        [primary_bytes.as_slice(), &vec![b' '; 64 * 1024]].concat(),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(life::managed_launch_path(&vars, 42, &a), None);
    std::fs::remove_file(&primary).unwrap_or_else(|err| panic!("{err}"));
    let other = root.path().join("other-primary");
    std::fs::write(&other, primary_bytes).unwrap_or_else(|err| panic!("{err}"));
    std::os::unix::fs::symlink(&other, &primary).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(life::managed_launch_path(&vars, 42, &a), None);

    let mut resume_vars = vars;
    resume_vars.push(("PATH".into(), "/bin".into()));
    resume_vars.retain(|(key, _)| !life::managed_launch_var(key));
    assert_eq!(
        resume_vars,
        vec![(OsString::from("PATH"), OsString::from("/bin"))]
    );
}

fn alab_package(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root.join("bin")).unwrap_or_else(|err| panic!("{err}"));
    let exe = executable(&root.join("bin"), "codex", 0o755);
    std::fs::write(
        root.join("codex-package.json"),
        r#"{"layoutVersion":1,"entrypoint":"bin/codex","version":"A"}"#,
    )
    .unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(root.join("alab-build.json"), "{}").unwrap_or_else(|err| panic!("{err}"));
    exe
}

fn package_root(exe: &Path) -> &Path {
    exe.parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("a package fixture"))
}

#[test]
fn a_managed_primary_must_reach_its_advertised_handoff() {
    let root = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let prefix = root.path().join("pkg");
    let entry = prefix.join("store/codex/exec/codex");
    let image = alab_package(&prefix.join("store/codex/101"));
    let primary = prefix.join("bin/codex");
    managed_primary(&primary, &entry, &image);
    let vars = fallback_vars(42, &entry, &image);
    assert_eq!(life::managed_launch_path(&vars, 42, &image), Some(entry));
    let produced = std::fs::read_to_string(&primary).unwrap_or_else(|err| panic!("{err}"));
    let after_header = produced
        .strip_prefix("#!/bin/sh\n")
        .unwrap_or_else(|| panic!("a produced shell header"));
    let rejected = ["exec '/other' \"$@\"", "exit 0"].map(|command| {
        std::fs::write(&primary, format!("#!/bin/sh\n{command}\n{after_header}"))
            .unwrap_or_else(|err| panic!("{err}"));
        life::managed_launch_path(&vars, 42, &image)
    });
    assert_eq!(rejected, [None, None]);
}

#[test]
fn a_store_primary_retains_only_its_complete_produced_prelude() {
    let root = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let prefix = root.path().join("pkg 'quoted");
    let entry = prefix.join("store/codex/exec/codex");
    let image = alab_package(&prefix.join("store/codex/101"));
    let primary = prefix.join("bin/codex");
    let vars = fallback_vars(42, &entry, &image);
    let atpkg = root.path().join("aterm 'bridge");
    // These are data-only exports accepted by the real producer, including shell-
    // looking bytes inside one literal value. They must not become commands.
    let exports = "# shim_env is aterm's policy for this program: only this managed copy runs with it.\nexport DISABLE_AUTOUPDATER='1'\nexport ATPKG_MANAGED_LAUNCH_EXTRA='kept'\nexport LITERAL='d'\\''Artagnan $(literal) # data'\n";
    for atpkg in [Path::new(""), atpkg.as_path()] {
        let prelude = store_prelude(&prefix, atpkg);
        managed_primary_with_prefix(&primary, &entry, &image, &format!("{prelude}{exports}"));
        assert_eq!(
            life::managed_launch_path(&vars, 42, &image),
            Some(entry.clone())
        );
        let middle_exit = prelude.replacen("__atpkg_yolo=0\n", "exit 0\n__atpkg_yolo=0\n", 1);
        assert_ne!(middle_exit, prelude);
        let partial = prelude.replacen("esac\n", "", 1);
        assert_ne!(partial, prelude);
        let other_prefix = store_prelude(&root.path().join("other"), atpkg);
        let unsafe_export = exports.replacen("'kept'", "'kept'; exit 0", 1);
        let corrupted = [
            format!("exec '/other' \"$@\"\n{prelude}{exports}"),
            format!("{middle_exit}{exports}"),
            format!("{partial}{exports}"),
            format!("{other_prefix}{exports}"),
            format!("{prelude}exit 0\n{exports}"),
            format!("{prelude}{unsafe_export}"),
        ];
        let rejected = corrupted.map(|body| {
            managed_primary_with_prefix(&primary, &entry, &image, &body);
            life::managed_launch_path(&vars, 42, &image)
        });
        assert_eq!(rejected, [None, None, None, None, None, None]);
    }
    // DevLink primaries have no Store prelude but preserve the same literal exports.
    managed_primary_with_prefix(&primary, &entry, &image, exports);
    assert_eq!(life::managed_launch_path(&vars, 42, &image), Some(entry));
}

fn store_prelude(prefix: &Path, atpkg: &Path) -> String {
    // Actual Store output captured by the existing atpkg aggregate. The two
    // bindings vary by installation; every produced statement stays intact.
    let template = include_str!("managed_primary_store_prelude.txt");
    let (head, rest) = template
        .split_once("{{ATPKG}}")
        .unwrap_or_else(|| panic!("an atpkg binding"));
    let (between, tail) = rest
        .split_once("{{PREFIX}}")
        .unwrap_or_else(|| panic!("a prefix binding"));
    format!(
        "{head}{}{between}{}{tail}",
        quote_path(atpkg),
        quote_path(prefix),
    )
}

fn fallback_vars(pid: u32, entry: &Path, target: &Path) -> Vec<(OsString, OsString)> {
    vec![
        ("ATPKG_MANAGED_LAUNCH_PID".into(), pid.to_string().into()),
        (
            "ATPKG_MANAGED_LAUNCH_PATH".into(),
            entry.as_os_str().to_owned(),
        ),
        (
            "ATPKG_MANAGED_LAUNCH_TARGET".into(),
            target.as_os_str().to_owned(),
        ),
    ]
}

fn managed_primary(primary: &Path, entry: &Path, target: &Path) {
    managed_primary_with_prefix(primary, entry, target, "");
}

fn quote_path(path: &Path) -> String {
    format!(
        "'{}'",
        path.to_str()
            .unwrap_or_else(|| panic!("a UTF-8 fixture path"))
            .replace('\'', "'\\''")
    )
}

fn managed_primary_with_prefix(primary: &Path, entry: &Path, target: &Path, prefix: &str) {
    std::fs::create_dir_all(primary.parent().unwrap_or_else(|| panic!("a parent")))
        .unwrap_or_else(|err| panic!("{err}"));
    let (entry, target) = (quote_path(entry), quote_path(target));
    std::fs::write(primary, format!("#!/bin/sh\n# atpkg shim — exec so the tool authenticates at its real path.\n{prefix}[ -L {entry} ] && [ -x {entry} ] && exec {entry} \"$@\"\nexport ATPKG_MANAGED_LAUNCH_PID=\"$$\"\nexport ATPKG_MANAGED_LAUNCH_PATH={entry}\nexport ATPKG_MANAGED_LAUNCH_TARGET={target}\nexec {target} \"$@\"\n"))
        .unwrap_or_else(|err| panic!("{err}"));
}
