// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native visualization/cache controls only; no Source/Native/file permission
//! is constructed by serialized paths, thread IDs or cached documents.
use super::*;
use pretty_assertions::assert_eq;

#[test]
fn actual_materialized_cache_aliases_require_a_real_native_policy_context() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let thread = codex_protocol::ThreadId::new();
    let original = InlineVisualizationContext::new(home.path(), thread).unwrap();
    std::fs::create_dir_all(&original.thread_dir)?;
    std::fs::write(
        original.thread_dir.join("chart.html"),
        "<main>actual original chart</main>",
    )?;
    let actual_url = original.link_for("chart.html").unwrap();
    let clone = original.clone();
    let separate = InlineVisualizationContext::new(home.path(), thread).unwrap();
    let mut capture = Capture::default();
    let first = capture.context(&original)?;
    let same = capture.context(&clone)?;
    let distinct = capture.context(&separate)?;
    assert_eq!(first.cache, same.cache);
    assert_ne!(first.cache, distinct.cache);
    let bytes = serde_json::to_vec(&capture.export()?).unwrap();
    let mut bindings = Bindings::default();
    bindings.install(serde_json::from_slice(&bytes).unwrap())?;
    assert!(bindings.context(&first).is_err());
    // This is the actual original constructor, not a context from the DTO's paths.
    let current = InlineVisualizationContext::new(home.path(), thread).unwrap();
    bindings.bind(&current)?;
    let a = bindings.context(&first)?;
    let b = bindings.context(&same)?;
    let c = bindings.context(&distinct)?;
    assert!(Arc::ptr_eq(
        &a.materialized_viewers,
        &b.materialized_viewers
    ));
    assert!(!Arc::ptr_eq(
        &a.materialized_viewers,
        &c.materialized_viewers
    ));
    assert!(!Arc::ptr_eq(
        &a.materialized_viewers,
        &current.materialized_viewers
    ));
    assert_eq!(
        *a.materialized_viewers.lock().unwrap(),
        *original.materialized_viewers.lock().unwrap()
    );
    assert_eq!(a.link_for("chart.html"), Some(actual_url));
    assert!(current.materialized_viewers.lock().unwrap().is_empty());
    Ok(())
}
#[test]
fn cache_paths_cannot_expand_a_bound_native_viewer_directory() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let original =
        InlineVisualizationContext::new(home.path(), codex_protocol::ThreadId::new()).unwrap();
    let mut capture = Capture::default();
    let reference = capture.context(&original)?;
    let mut data = capture.export()?;
    data.caches[0].push(Entry {
        path: PathData::capture(Path::new("/foreign/chart.html")),
        document: "untrusted cached document".into(),
    });
    let mut bindings = Bindings::default();
    bindings.bind(&original)?;
    bindings.install(data)?;
    assert!(bindings.context(&reference).is_err());
    assert!(original.materialized_viewers.lock().unwrap().is_empty());
    let mut reference = reference;
    reference.viewer_dir = PathData::capture(Path::new("/foreign/viewer"));
    assert!(bindings.context(&reference).is_err());
    Ok(())
}
#[test]
fn duplicate_cache_entries_and_closed_graph_fields_are_refused() -> io::Result<()> {
    let home = tempfile::tempdir()?;
    let original =
        InlineVisualizationContext::new(home.path(), codex_protocol::ThreadId::new()).unwrap();
    let mut capture = Capture::default();
    capture.context(&original)?;
    let mut data = capture.export()?;
    let path = original.viewer_dir.join(".codex-viewers/chart.html");
    data.caches[0].push(Entry {
        path: PathData::capture(&path),
        document: "first".into(),
    });
    data.caches[0].push(Entry {
        path: PathData::capture(&path),
        document: "second".into(),
    });
    assert!(Bindings::default().install(data).is_err());
    assert!(
        serde_json::from_slice::<GraphData>(br#"{"schema":1,"caches":[],"allowedRoot":"/"}"#)
            .is_err()
    );
    Ok(())
}
