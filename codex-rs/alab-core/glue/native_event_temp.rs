// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private scratch for a fixture that must receive real native file events.
//! Ordinary test scratch remains under `.ubd`; FSEvents intentionally ignores it.

use std::ffi::OsStr;
use std::path::Path;

fn event_root(temp: &Path) -> &Path {
    let mut root = temp;
    for ancestor in temp.ancestors() {
        if ancestor.file_name() == Some(OsStr::new(".ubd"))
            && let Some(parent) = ancestor.parent()
        {
            root = parent;
        }
    }
    root
}

fn directory_in(temp: &Path) -> std::io::Result<tempfile::TempDir> {
    let physical = std::fs::canonicalize(temp)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix("codex-event-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir_in(event_root(&physical))
}

pub(super) fn directory() -> std::io::Result<tempfile::TempDir> {
    directory_in(&std::env::temp_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_root_leaves_every_excluded_component_without_changing_other_names() {
        assert_eq!(
            event_root(Path::new("/private/tmp/.ubd/runtime/.ubd/child")),
            Path::new("/private/tmp")
        );
        assert_eq!(
            event_root(Path::new("/private/tmp/not.ubd/child")),
            Path::new("/private/tmp/not.ubd/child")
        );
    }

    #[cfg(unix)]
    #[test]
    fn event_directory_is_private_outside_excluded_ancestors_and_cleans_up() {
        use std::os::unix::fs::PermissionsExt;

        let source = tempfile::tempdir().expect("fixture scratch");
        let nested = source.path().join(".ubd/inside/.ubd/leaf");
        std::fs::create_dir_all(&nested).expect("excluded scratch");
        let directory = directory_in(&nested).expect("event-visible private scratch");
        let path = directory.path().to_path_buf();
        assert!(
            !path
                .ancestors()
                .any(|part| part.file_name() == Some(OsStr::new(".ubd")))
        );
        assert_eq!(
            std::fs::metadata(&path)
                .expect("private scratch metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        drop(directory);
        assert!(!path.exists());
    }
}
