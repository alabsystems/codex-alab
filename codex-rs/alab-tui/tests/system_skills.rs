// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! codex-alab's embedded system skills sit in `skills/.alab-system`, beside the `.system`
//! folder `codex-old` (OpenAI's own build, sharing `~/.codex`) keeps (`ALAB(skills)`). The
//! switch is on in this test binary, as in the codex binary; `inactive.rs` holds the run
//! without it.

use std::fs;

use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::ConfigRequirementsToml;
use codex_config::skill_config_rules_from_stack;
use codex_core::config::edit::ConfigEdit;
use codex_core::config::edit::apply_blocking;
use codex_skills::install_system_skills;
use codex_skills::system_cache_root_dir;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

/// The install lays codex-alab's set in its own folder and leaves codex-old's set, marker and
/// all, as it was. NEGATIVE CONTROL: without the hook the root is `.system` and the folder
/// assertion fails; past it, the install would clear codex-old's set (its marker names another).
#[test]
fn the_install_leaves_codex_olds_skills_alone() {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let home = AbsolutePathBuf::from_absolute_path(dir.path())
        .unwrap_or_else(|err| panic!("absolute scratch dir: {err}"));
    let old = dir.path().join("skills").join(".system");
    fs::create_dir_all(old.join("imagegen")).unwrap_or_else(|err| panic!("{err}"));
    fs::write(old.join(".codex-system-skills.marker"), "openai\n")
        .unwrap_or_else(|err| panic!("{err}"));
    fs::write(old.join("imagegen").join("SKILL.md"), "codex-old's\n")
        .unwrap_or_else(|err| panic!("{err}"));

    let root = system_cache_root_dir(&home);
    assert_eq!(
        root.as_path(),
        dir.path().join("skills").join(".alab-system")
    );
    install_system_skills(&home).unwrap_or_else(|err| panic!("install: {err}"));
    assert!(
        root.join("review-agent")
            .join("SKILL.md")
            .as_path()
            .is_file()
    );
    assert!(root.join(".codex-system-skills.marker").as_path().is_file());
    for skill in ["imagegen", "openai-docs", "skill-installer"] {
        assert!(root.join(skill).join("SKILL.md").as_path().is_file());
    }
    // A second launch finds its marker and writes nothing: a re-lay clears the folder first.
    let sentinel = root.join("sentinel");
    fs::write(sentinel.as_path(), "").unwrap_or_else(|err| panic!("{err}"));
    install_system_skills(&home).unwrap_or_else(|err| panic!("install again: {err}"));
    assert!(sentinel.as_path().is_file());

    assert_eq!(
        fs::read_to_string(old.join(".codex-system-skills.marker")).ok(),
        Some("openai\n".to_string())
    );
    assert_eq!(
        fs::read_to_string(old.join("imagegen").join("SKILL.md")).ok(),
        Some("codex-old's\n".to_string())
    );
}

/// One `[[skills.config]]` rule in the shared `config.toml` turns a system skill off in both
/// builds: codex-alab saves its toggle under codex-old's `.system` path, and reads a `.system`
/// rule (its own, or one codex-old saved) as naming its `.alab-system` copy; turning the skill
/// back on finds that rule and removes it. NEGATIVE CONTROL: without the write hook the saved
/// path names `.alab-system`, which codex-old never loads (the first assertion fails); without
/// the read hook the rule disables only a `.system` path codex-alab never loads (the second).
#[test]
fn a_system_skill_turned_off_in_either_build_is_off_in_both() {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let home = AbsolutePathBuf::from_absolute_path(dir.path())
        .unwrap_or_else(|err| panic!("absolute scratch dir: {err}"));
    install_system_skills(&home).unwrap_or_else(|err| panic!("install: {err}"));
    let own = system_cache_root_dir(&home)
        .join("skill-creator")
        .join("SKILL.md")
        .canonicalize()
        .unwrap_or_else(|err| panic!("codex-alab's skill-creator: {err}"));
    let canonical_home = dir
        .path()
        .canonicalize()
        .unwrap_or_else(|err| panic!("{err}"));
    let shared = canonical_home.join("skills/.system/skill-creator/SKILL.md");
    let toggle = |enabled| {
        apply_blocking(
            dir.path(),
            &[ConfigEdit::SetSkillConfig {
                path: own.to_path_buf(),
                enabled,
            }],
        )
        .unwrap_or_else(|err| panic!("save the toggle: {err}"));
        let saved = fs::read_to_string(dir.path().join("config.toml")).unwrap_or_default();
        toml::from_str::<toml::Value>(&saved).unwrap_or_else(|err| panic!("{err}: {saved}"))
    };

    let saved = toggle(false);
    assert_eq!(
        saved["skills"]["config"][0]["path"].as_str(),
        Some(shared.to_string_lossy().as_ref())
    );
    let user = AbsolutePathBuf::from_absolute_path(dir.path().join("config.toml"))
        .unwrap_or_else(|err| panic!("{err}"));
    let stack = ConfigLayerStack::new(
        vec![ConfigLayerEntry::new(
            ConfigLayerSource::User {
                file: user,
                profile: None,
            },
            saved,
        )],
        Default::default(),
        ConfigRequirementsToml::default(),
    )
    .unwrap_or_else(|err| panic!("config stack: {err}"));
    let own_uri = PathUri::from_abs_path(&own);
    let disabled =
        skill_config_rules_from_stack(&stack).resolve_disabled_paths([("skill-creator", &own_uri)]);
    assert!(disabled.contains(&own_uri), "{disabled:?}");

    assert_eq!(toggle(true).get("skills"), None);
}

/// The app-server's skills watcher passes over codex-old's `skills/.system` as it does over
/// codex-alab's own folder, and nothing else. NEGATIVE CONTROL: with the switch off the helper is
/// false, and the watcher reloads every client's skills when codex-old re-lays its set.
#[test]
fn codex_olds_reinstall_is_no_skill_change() {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let home = AbsolutePathBuf::from_absolute_path(dir.path())
        .unwrap_or_else(|err| panic!("absolute scratch dir: {err}"));
    let root = system_cache_root_dir(&home);
    let skills = dir.path().join("skills");
    let other = |path: &str| {
        codex_alab_core::home::in_other_system_skills(root.as_path(), &skills.join(path))
    };
    assert!(other(".system/imagegen/SKILL.md"));
    assert!(other(".system/.codex-system-skills.marker"));
    assert!(!other("my-skill/SKILL.md"));
    assert!(!other(".systemd/SKILL.md"));
}
