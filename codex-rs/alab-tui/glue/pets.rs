// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(pets) glue, compiled inside codex-tui as `pets::catalog::alab_pets` (one `#[path]`
//! line at the end of `tui/src/pets/catalog.rs`): ALab's built-in pets, listed in `/pets`
//! beside upstream's, and their sheets drawn into codex's built-in cache from code in the binary
//! (`codex_alab_tui::pets::pack`) instead of downloaded from OpenAI's CDN.
//!
//! Three hooks reach it: `catalog::builtin_pet` and the `/pets` picker read [`builtins`];
//! `asset_pack::ensure_builtin_pet` asks [`materialize`] first. Outside the codex binary
//! (`codex_alab_core::active` off, upstream's tests included) [`builtins`] is upstream's list
//! and [`materialize`] declines every pet.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::Context;
use anyhow::Result;
use codex_alab_tui::pets::pack;

use super::BUILTIN_PETS;
use super::BuiltinPet;
use super::SPRITESHEET_HEIGHT;
use super::SPRITESHEET_WIDTH;

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

/// Make sure ALab's pet `pet` has its sheet in codex's built-in cache: `Ok(true)` when it is
/// one of ALab's and the sheet is in place (drawn now if missing or the wrong size), `Ok(false)`
/// for any other pet, which upstream then fetches as before.
pub(in crate::pets) async fn materialize(codex_home: &Path, pet: BuiltinPet) -> Result<bool> {
    if !codex_alab_tui::pets::active() || !ALAB_PETS.contains(&pet) {
        return Ok(false);
    }
    let destination = crate::pets::builtin_spritesheet_path(codex_home, pet.spritesheet_file);
    tokio::task::spawn_blocking(move || draw_sheet(&destination))
        .await
        .context("join ALab pet sheet task")??;
    Ok(true)
}

/// Draw ALab's sheet at `destination` unless a sheet of the right size is already there;
/// written to a staging file and renamed into place, so a reader never sees half a sheet.
fn draw_sheet(destination: &Path) -> Result<()> {
    let valid = image::image_dimensions(destination)
        .is_ok_and(|size| size == (SPRITESHEET_WIDTH, SPRITESHEET_HEIGHT));
    if valid {
        return Ok(());
    }
    let parent = destination
        .parent()
        .context("pet spritesheet path should include an assets directory")?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let image = image::RgbaImage::from_raw(
        SPRITESHEET_WIDTH,
        SPRITESHEET_HEIGHT,
        pack::spritesheet_rgba(),
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
        draw_sheet(&path).unwrap();
        let decoded = image::open(&path).unwrap().to_rgba8();
        assert_eq!(
            decoded.dimensions(),
            (SPRITESHEET_WIDTH, SPRITESHEET_HEIGHT)
        );
        assert!(decoded.into_raw() == pack::spritesheet_rgba());

        // A valid sheet stays as it is; a wrong one is drawn again.
        let stamp = fs::metadata(&path).unwrap().modified().unwrap();
        draw_sheet(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), stamp);
        fs::write(&path, b"not a png").unwrap();
        draw_sheet(&path).unwrap();
        assert_eq!(
            image::image_dimensions(&path).unwrap(),
            (SPRITESHEET_WIDTH, SPRITESHEET_HEIGHT)
        );
        let leftovers = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no staging file is left behind");
    }

    #[tokio::test]
    async fn upstreams_pets_are_left_to_upstream() {
        let home = tempfile::tempdir().unwrap();
        let dewey = super::super::builtin_pet("dewey").unwrap();
        assert!(!materialize(home.path(), dewey).await.unwrap());
    }
}
