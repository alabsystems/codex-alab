// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real local marker identity only; no clipboard/worker/Native authority.
use super::*;
#[test]
fn actual_ui_roots_and_worker_weak_scopes_share_one_graph_without_value_merging() -> io::Result<()>
{
    let selection = Arc::new(());
    let other = Arc::new(());
    let mut capture = ScopeCapture::default();
    assert!(capture.weak(&Arc::downgrade(&selection)).is_err());
    let selected = capture.strong(&selection)?;
    let repeated = capture.strong(&selection)?;
    let distinct = capture.strong(&other)?;
    assert_eq!(selected, repeated);
    assert_ne!(selected, distinct);
    let weak = capture.weak(&Arc::downgrade(&selection))?;
    let bytes = serde_json::to_vec(&capture.export()?).map_err(invalid)?;
    let restored = ScopeRestored::new(serde_json::from_slice(&bytes).map_err(invalid)?)?;
    assert!(restored.validate_roots().is_err());
    let selection = restored.strong(selected)?;
    let same = restored.strong(repeated)?;
    let other = restored.strong(distinct)?;
    let weak = restored.weak(weak)?;
    assert!(Arc::ptr_eq(&selection, &same));
    assert!(!Arc::ptr_eq(&selection, &other));
    assert!(Weak::ptr_eq(&weak, &Arc::downgrade(&selection)));
    restored.validate_roots()?;
    drop(restored);
    drop(selection);
    drop(same);
    assert!(weak.upgrade().is_none());
    Ok(())
}
#[test]
fn expired_missing_and_malformed_scope_data_cannot_manufacture_ui_roots() -> io::Result<()> {
    let live = Arc::new(());
    let weak = Arc::downgrade(&live);
    drop(live);
    let capture = ScopeCapture::default();
    assert!(capture.weak(&weak)?.is_none());
    let restored = ScopeRestored::new(capture.export()?)?;
    assert!(restored.weak(None)?.upgrade().is_none());
    let bad: ScopeRef = serde_json::from_value(serde_json::json!(1_000_001)).map_err(invalid)?;
    assert!(restored.strong(bad).is_err());
    assert!(restored.weak(Some(bad)).is_err());
    for bad in [
        serde_json::json!({"schema":1}),
        serde_json::json!({"schema":1,"count":0,"nativeLease":true}),
    ] {
        assert!(serde_json::from_value::<ScopeData>(bad).is_err());
    }
    assert!(
        ScopeRestored::new(ScopeData {
            schema: 1,
            count: 1_000_001
        })
        .is_err()
    );
    Ok(())
}
