// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(pets) glue, compiled inside codex-tui as `pets::catalog::alab_pets` (one `#[path]`
//! line at the end of `tui/src/pets/catalog.rs`): ALab's built-in pets, listed in `/pets`
//! beside upstream's, and their sheets drawn into codex's built-in cache from code in the binary
//! (`codex_alab_tui::pets::pack`) instead of downloaded from OpenAI's CDN.
//!
//! Five hooks reach it: `catalog::builtin_pet` and the `/pets` picker read [`builtins`];
//! `asset_pack::ensure_builtin_pet` asks [`materialize`] first; the picker's custom-pet scan
//! passes over what [`shared_copy`] names; the model asks [`load`] for ALab geometry and timing.
//! Outside the codex binary
//! (`codex_alab_core::active` off, upstream's tests included) [`builtins`] is upstream's list,
//! [`materialize`] and [`load`] decline every pet and [`shared_copy`] names none.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use codex_alab_tui::pets::pack;

use super::BUILTIN_PETS;
use super::BuiltinPet;
use crate::pets::model::Animation;
use crate::pets::model::AnimationFrame;
use crate::pets::model::Pet;

/// ALab's pets.
const ALAB_PETS: [BuiltinPet; 1] = [BuiltinPet {
    id: pack::KITTY_ID,
    display_name: pack::KITTY_NAME,
    description: pack::KITTY_DESCRIPTION,
    spritesheet_file: pack::KITTY_FILE,
}];

/// Every built-in pet: ALab's first, then upstream's (upstream's alone when not active).
pub(in crate::pets) fn builtins() -> &'static [BuiltinPet] {
    builtins_when(codex_alab_tui::pets::active())
}

fn builtins_when(active: bool) -> &'static [BuiltinPet] {
    static ALL: LazyLock<Vec<BuiltinPet>> =
        LazyLock::new(|| ALAB_PETS.iter().chain(BUILTIN_PETS).copied().collect());
    if active { &ALL } else { BUILTIN_PETS }
}

/// Load ALab's sheet with its own geometry and evenly paced animation tracks.
pub(in crate::pets) fn load(pet: BuiltinPet, codex_home: Option<&Path>) -> Result<Option<Pet>> {
    load_when(codex_alab_tui::pets::active(), pet, codex_home)
}

fn load_when(active: bool, pet: BuiltinPet, codex_home: Option<&Path>) -> Result<Option<Pet>> {
    if !active || !ALAB_PETS.contains(&pet) {
        return Ok(None);
    }
    let codex_home = codex_home.context("CODEX_HOME is not available")?;
    let spritesheet_path = crate::pets::builtin_spritesheet_path(codex_home, pet.spritesheet_file);
    anyhow::ensure!(
        spritesheet_path.exists(),
        "missing spritesheet {}",
        spritesheet_path.display()
    );
    Ok(Some(Pet {
        id: pet.id.into(),
        display_name: pet.display_name.into(),
        description: pet.description.into(),
        spritesheet_path,
        frame_width: pack::FRAME_W as u32,
        frame_height: pack::FRAME_H as u32,
        columns: pack::COLUMNS as u32,
        rows: pack::ROWS as u32,
        frame_count: pack::COLUMNS * pack::ROWS,
        animations: animations(pack::COLUMNS),
    }))
}

fn animations(columns: usize) -> HashMap<String, Animation> {
    let mut animations: HashMap<_, _> = pack::ROW_NAMES
        .iter()
        .enumerate()
        .map(|(row, name)| {
            (
                (*name).to_owned(),
                Animation {
                    frames: (0..columns)
                        .map(|column| AnimationFrame {
                            sprite_index: row * columns + column,
                            duration: Duration::from_millis(
                                pack::ROW_FRAME_MS[row] * pack::COLUMNS as u64 / columns as u64,
                            ),
                        })
                        .collect(),
                    loop_start: (row != 5).then_some(0),
                    fallback: "idle".into(),
                },
            )
        })
        .collect();
    for (alias, name) in [
        ("move_right", "running-right"),
        ("move_left", "running-left"),
        ("wave", "waving"),
        ("bounce", "jumping"),
        ("sad", "failed"),
    ] {
        animations.insert(alias.into(), animations[name].clone());
    }
    animations
}

/// Make sure ALab's pet `pet` has its sheet in codex's built-in cache: `Ok(true)` when it is
/// one of ALab's and the sheet is in place (drawn now if missing or the wrong size), `Ok(false)`
/// for any other pet, which upstream then fetches as before. codex-old gets its copy too
/// ([`share_with_codex_old`]); failing that leaves codex-alab's pet as it is.
pub(in crate::pets) async fn materialize(codex_home: &Path, pet: BuiltinPet) -> Result<bool> {
    if !codex_alab_tui::pets::active() || !ALAB_PETS.contains(&pet) {
        return Ok(false);
    }
    let destination = crate::pets::builtin_spritesheet_path(codex_home, pet.spritesheet_file);
    let codex_home = codex_home.to_path_buf();
    tokio::task::spawn_blocking(move || {
        draw_sheet(&destination, pack::COLUMNS)?;
        if let Err(err) = share_with_codex_old(&codex_home, pet) {
            tracing::warn!("lay {} as codex-old's custom pet: {err:#}", pet.id);
        }
        anyhow::Ok(())
    })
    .await
    .context("join ALab pet sheet task")??;
    Ok(true)
}

/// Whether the custom pet `id` in `CODEX_HOME/pets` is codex-old's copy of one of ALab's,
/// which codex-alab's `/pets` already lists as the built-in (upstream's custom-pet scan asks).
pub(in crate::pets) fn shared_copy(id: &str) -> bool {
    shared_copy_when(codex_alab_tui::pets::active(), id)
}

fn shared_copy_when(active: bool, id: &str) -> bool {
    active && ALAB_PETS.iter().any(|pet| pet.id == id)
}

/// The sheet's name in codex-old's copy of an ALab pet.
const SHARED_SHEET: &str = "spritesheet.png";

/// Lay ALab's pet `pet` down as a custom pet (`CODEX_HOME/pets/<id>`, a manifest and its sheet)
/// unless that folder already has a manifest. codex-alab and `codex-old` share `config.toml`, so
/// choosing the kitty in codex-alab sets `[tui] pet = "alab-kitty"` for both; `codex-old` has no
/// such built-in and looks the id up there, and without this copy it warned "Failed to load
/// configured pet: unknown pet alab-kitty" at every launch and drew no pet.
fn share_with_codex_old(codex_home: &Path, pet: BuiltinPet) -> Result<()> {
    let folder = codex_home.join("pets").join(pet.id);
    let manifest = folder.join("pet.json");
    if manifest.is_file() {
        return Ok(());
    }
    fs::create_dir_all(&folder).with_context(|| format!("create {}", folder.display()))?;
    draw_sheet(&folder.join(SHARED_SHEET), pack::LEGACY_COLUMNS)?;
    let body = serde_json::to_vec_pretty(&serde_json::json!({
        "id": pet.id,
        "displayName": pet.display_name,
        "description": pet.description,
        "spritesheetPath": SHARED_SHEET,
        "frame": {
            "width": pack::FRAME_W,
            "height": pack::FRAME_H,
            "columns": pack::LEGACY_COLUMNS,
            "rows": pack::ROWS,
        },
        "animations": animations(pack::LEGACY_COLUMNS).into_iter().map(|(name, animation)| {
            let spec = serde_json::json!({
                "frames": animation.frames.iter().map(|frame| frame.sprite_index).collect::<Vec<_>>(),
                "fps": 1000.0 / animation.frames[0].duration.as_millis() as f64,
                "loop": animation.loop_start.is_some(),
                "fallback": animation.fallback,
            });
            (name, spec)
        }).collect::<serde_json::Map<_, _>>(),
    }))?;
    let staging = folder.join(format!(".pet.json.lay-{}", uuid::Uuid::new_v4()));
    fs::write(&staging, body).with_context(|| format!("write {}", staging.display()))?;
    fs::rename(&staging, &manifest).with_context(|| {
        let _ = fs::remove_file(&staging);
        format!("install {}", manifest.display())
    })
}

/// Draw ALab's sheet at `destination` unless a sheet of the right size is already there;
/// written to a staging file and renamed into place, so a reader never sees half a sheet.
fn draw_sheet(destination: &Path, columns: usize) -> Result<()> {
    let width = (pack::FRAME_W * columns) as u32;
    let valid = image::image_dimensions(destination)
        .is_ok_and(|size| size == (width, pack::SHEET_H as u32));
    if valid {
        return Ok(());
    }
    let parent = destination
        .parent()
        .context("pet spritesheet path should include an assets directory")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let image = image::RgbaImage::from_raw(
        width,
        pack::SHEET_H as u32,
        if columns == pack::COLUMNS {
            pack::spritesheet_rgba()
        } else {
            pack::legacy_spritesheet_rgba()
        },
    )
    .context("ALab pet sheet geometry")?;
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("alab-pet.png");
    let staging =
        destination.with_file_name(format!(".{file_name}.draw-{}.png", uuid::Uuid::new_v4()));
    image
        .save_with_format(&staging, image::ImageFormat::Png)
        .with_context(|| format!("write {}", staging.display()))?;
    fs::rename(&staging, destination).with_context(|| {
        let _ = fs::remove_file(&staging);
        format!("install {}", destination.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn alabs_pets_lead_and_never_shadow_upstreams() {
        let all = builtins_when(true);
        assert_eq!(all[0].id, "alab-kitty");
        assert_eq!(&all[ALAB_PETS.len()..], BUILTIN_PETS);
        for pet in ALAB_PETS {
            assert!(BUILTIN_PETS.iter().all(|upstream| upstream.id != pet.id));
            assert!(
                BUILTIN_PETS
                    .iter()
                    .all(|upstream| upstream.spritesheet_file != pet.spritesheet_file)
            );
        }
    }

    #[test]
    fn upstreams_list_stands_when_not_active() {
        assert_eq!(builtins_when(false), BUILTIN_PETS);
    }

    #[test]
    fn the_sheet_is_drawn_once_into_the_builtin_cache_and_decodes_to_the_pack() {
        let home = tempfile::tempdir().unwrap();
        let path = crate::pets::builtin_spritesheet_path(home.path(), pack::KITTY_FILE);
        draw_sheet(&path, pack::COLUMNS).unwrap();
        let decoded = image::open(&path).unwrap().to_rgba8();
        assert_eq!(
            decoded.dimensions(),
            (pack::SHEET_W as u32, pack::SHEET_H as u32)
        );
        assert!(decoded.into_raw() == pack::spritesheet_rgba());

        // A valid sheet stays as it is; a wrong one is drawn again.
        let stamp = fs::metadata(&path).unwrap().modified().unwrap();
        draw_sheet(&path, pack::COLUMNS).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), stamp);
        fs::write(&path, b"not a png").unwrap();
        draw_sheet(&path, pack::COLUMNS).unwrap();
        assert_eq!(
            image::image_dimensions(&path).unwrap(),
            (pack::SHEET_W as u32, pack::SHEET_H as u32)
        );
        let leftovers = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no staging file is left behind");
    }

    /// codex-old loads the copy codex-alab lays when it draws the kitty, under the id their
    /// shared `config.toml` names: in this test binary the switch is off, so, as in codex-old,
    /// ALab's pets are no built-ins. A copy already there is left alone. NEGATIVE CONTROL:
    /// without the copy the load fails ("unknown pet alab-kitty").
    #[test]
    fn codex_old_loads_the_kitty_chosen_in_codex_alab() {
        let home = tempfile::tempdir().unwrap();
        let kitty = ALAB_PETS[0];
        let load = || crate::pets::model::Pet::load_with_codex_home(kitty.id, Some(home.path()));
        assert!(load().is_err());
        share_with_codex_old(home.path(), kitty).unwrap();
        let pet = load().unwrap();
        assert_eq!(pet.display_name, kitty.display_name);
        assert_eq!(pet.description, kitty.description);
        assert_eq!((pet.columns, pet.rows, pet.frame_count), (8, 9, 72));
        for (row, name) in pack::ROW_NAMES.iter().enumerate() {
            let track = &pet.animations[*name];
            assert_eq!(track.frames.len(), 8);
            assert_eq!(
                track.frames[0].duration.as_millis(),
                u128::from(pack::ROW_FRAME_MS[row] * 3)
            );
        }

        let manifest = home.path().join("pets").join(kitty.id).join("pet.json");
        let mut renamed: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        renamed["displayName"] = "Renamed".into();
        fs::write(&manifest, renamed.to_string()).unwrap();
        share_with_codex_old(home.path(), kitty).unwrap();
        assert_eq!(load().unwrap().display_name, "Renamed");
    }

    #[test]
    fn smooth_builtin_tracks_match_the_extracted_sheet_and_even_cadence() {
        let home = tempfile::tempdir().unwrap();
        let builtin = ALAB_PETS[0];
        assert!(load_when(false, builtin, None).unwrap().is_none());
        assert!(load_when(true, BUILTIN_PETS[0], None).unwrap().is_none());
        let path = crate::pets::builtin_spritesheet_path(home.path(), builtin.spritesheet_file);
        draw_sheet(&path, pack::COLUMNS).unwrap();
        let pet = load_when(true, builtin, Some(home.path()))
            .unwrap()
            .unwrap();
        let directory = home.path().join("extracted");
        let frames = crate::pets::frames::prepare_png_frames(&pet, &directory).unwrap();
        assert_eq!(frames.len(), 216);
        assert!(pet.frame_count <= 256, "native pet frame limit");
        assert_eq!(image::image_dimensions(&frames[215]).unwrap(), (192, 208));
        for (row, name) in pack::ROW_NAMES.iter().enumerate() {
            let animation = &pet.animations[*name];
            assert_eq!(animation.frames.len(), 24);
            assert_eq!(animation.loop_start, (row != 5).then_some(0));
            for (column, frame) in animation.frames.iter().enumerate() {
                assert_eq!(frame.sprite_index, row * 24 + column);
                assert_eq!(
                    frame.duration,
                    Duration::from_millis(pack::ROW_FRAME_MS[row])
                );
            }
        }
    }

    /// codex-alab's `/pets` lists the kitty once, as the built-in; codex-old (and every upstream
    /// test, the switch off) lists the copy as a custom pet.
    #[test]
    fn the_copy_lists_once_in_codex_alab() {
        assert!(shared_copy_when(true, pack::KITTY_ID));
        assert!(!shared_copy_when(false, pack::KITTY_ID));
        assert!(!shared_copy_when(true, "chefito"));
    }

    #[tokio::test]
    async fn upstreams_pets_are_left_to_upstream() {
        let home = tempfile::tempdir().unwrap();
        let dewey = super::super::builtin_pet("dewey").unwrap();
        assert!(!materialize(home.path(), dewey).await.unwrap());
    }
}
