// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Watch rename locations along the launch chain, not just its first link.
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

pub fn directories(launch: &Path) -> Vec<PathBuf> {
    let mut directories = BTreeSet::new();
    if let Some(image) = resolve(launch, &mut directories)
        && let Some(head) = super::read_head(&image, 64 * 1024 + 1)
        && head.len() <= 64 * 1024
        && head.starts_with(b"#!")
        && let Ok(text) = std::str::from_utf8(&head)
        && let Some(target) = super::shim_target(text)
    {
        resolve(&target, &mut directories);
    }
    directories.into_iter().collect()
}

fn parent(path: &Path, directories: &mut BTreeSet<PathBuf>) {
    if let Some(parent) = path.parent().and_then(|parent| parent.canonicalize().ok())
        && parent.is_dir()
    {
        directories.insert(parent);
    }
}

fn resolve(path: &Path, directories: &mut BTreeSet<PathBuf>) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut current = path.to_path_buf();
    let mut seen = HashSet::new();
    for _ in 0..32 {
        if !seen.insert(current.clone()) {
            return None;
        }
        let mut prefix = PathBuf::new();
        let mut components = current.components();
        let mut next = None;
        while let Some(component) = components.next() {
            prefix.push(component.as_os_str());
            let metadata = match std::fs::symlink_metadata(&prefix) {
                Ok(metadata) => metadata,
                Err(_) => {
                    // Watch the last existing parent so a repaired/missing link
                    // prompts a fresh chain registration too.
                    parent(&prefix, directories);
                    return None;
                }
            };
            if metadata.is_symlink() {
                parent(&prefix, directories);
                let target = std::fs::read_link(&prefix).ok()?;
                let target = if target.is_absolute() {
                    target
                } else {
                    prefix.parent()?.join(target)
                };
                let remainder = components.as_path();
                next = Some(if remainder.as_os_str().is_empty() {
                    target
                } else {
                    target.join(remainder)
                });
                break;
            }
        }
        match next {
            Some(path) => current = path,
            None => {
                parent(&current, directories);
                return current.canonicalize().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn final_file_symlink_preserves_the_shim_target_watch() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        for directory in ["launch", "shims", "images"] {
            std::fs::create_dir(root.join(directory)).unwrap();
        }
        let image = root.join("images/codex");
        std::fs::write(&image, b"image").unwrap();
        let shim = root.join("shims/codex");
        std::fs::write(
            &shim,
            format!("#!/bin/sh\nexec '{}' \"$@\"\n", image.display()),
        )
        .unwrap();
        let launch = root.join("launch/codex");
        symlink(&shim, &launch).unwrap();
        let mut watched = BTreeSet::new();
        assert_eq!(resolve(&launch, &mut watched), Some(shim));
        assert!(directories(&launch).contains(&root.join("images")));
    }

    #[test]
    fn watches_relative_links_directory_links_and_shim_targets() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        for directory in ["launch", "route", "store", "images"] {
            std::fs::create_dir(root.join(directory)).unwrap();
        }
        std::fs::write(root.join("images/codex"), b"image").unwrap();
        std::fs::write(
            root.join("store/shim"),
            format!(
                "#!/bin/sh\nexec '{}' \"$@\"\n",
                root.join("images/codex").display()
            ),
        )
        .unwrap();
        symlink("../store", root.join("route/current")).unwrap();
        symlink("../route/current/shim", root.join("launch/codex")).unwrap();
        let watched = directories(&root.join("launch/codex"));
        for directory in ["launch", "route", "store", "images"] {
            assert!(watched.contains(&root.join(directory)), "{watched:?}");
        }
    }

    #[test]
    fn broken_and_cyclic_links_remain_bounded_and_watch_their_repair() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("launch")).unwrap();
        std::fs::create_dir(root.join("route")).unwrap();
        symlink("../route/missing/codex", root.join("launch/codex")).unwrap();
        let watched = directories(&root.join("launch/codex"));
        assert!(watched.contains(&root.join("launch")));
        assert!(watched.contains(&root.join("route")));
        std::fs::remove_file(root.join("launch/codex")).unwrap();
        symlink("codex", root.join("launch/codex")).unwrap();
        assert_eq!(
            directories(&root.join("launch/codex")),
            vec![root.join("launch")]
        );
    }
}
