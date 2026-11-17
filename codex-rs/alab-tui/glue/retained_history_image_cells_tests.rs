// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native cell DATA only; public restoration still requires genuine bound
//! renderer. No source path is opened, no SDK/Native resource is constructed.
use super::*;
#[test]
fn viewed_and_generated_original_fields_round_trip_without_path_reopen() -> io::Result<()> {
    let viewed = ViewImageHistoryCell {
        activity_id: "original-view-id".into(),
        filename: "view.png".into(),
        path_label: "/historical/missing/view.png".into(),
        link: None,
    };
    let mut graph = native::Capture::default();
    let data = Data::capture(&viewed, &mut graph, 0)?
        .ok_or_else(|| native::invalid("view native cell missing"))?;
    let restored = data.restore_data()?;
    let restored = restored
        .as_any()
        .downcast_ref::<ViewImageHistoryCell>()
        .ok_or_else(|| native::invalid("view native type changed"))?;
    assert_eq!(restored.activity_id, viewed.activity_id);
    assert_eq!(restored.filename, viewed.filename);
    assert_eq!(restored.path_label, viewed.path_label);
    assert!(restored.link.is_none());
    let generated = ImageGenerationHistoryCell {
        failed: false,
        lines: vec![Line::from("native generated message").bold()],
        saved_path: Some(
            AbsolutePathBuf::try_from(PathBuf::from("/historical/missing/generated.png"))
                .map_err(native::invalid)?,
        ),
    };
    let data = Data::capture(&generated, &mut graph, 0)?
        .ok_or_else(|| native::invalid("generated native cell missing"))?;
    let restored = data.restore_data()?;
    let restored = restored
        .as_any()
        .downcast_ref::<ImageGenerationHistoryCell>()
        .ok_or_else(|| native::invalid("generated native type changed"))?;
    assert_eq!(restored.lines, generated.lines);
    assert_eq!(restored.saved_path, generated.saved_path);
    assert!(!restored.failed);
    Ok(())
}
#[test]
fn failed_generation_cannot_acquire_an_image_path_and_saved_observations_are_required()
-> io::Result<()> {
    let wrong = Data::Generated {
        failed: true,
        lines: Vec::new(),
        saved_path: Some(b"/not-created.png".to_vec()),
    };
    assert!(wrong.restore_data().is_err());
    let wrong = serde_json::json!({"kind":"generated","failed":false,"lines":[]});
    assert!(serde_json::from_value::<Data>(wrong).is_err());
    let wrong =
        serde_json::json!({"kind":"viewed","activity_id":"x","filename":"x","path_label":"/x"});
    assert!(serde_json::from_value::<Data>(wrong).is_err());
    Ok(())
}
