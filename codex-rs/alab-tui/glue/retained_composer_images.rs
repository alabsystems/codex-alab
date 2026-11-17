// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native cache DATA, including actual unavailable entries. Decode never
//! stats a saved path or calls load_tile/for_terminal/render.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::retained_arc_graph::Id;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::sync::Mutex;
const MAX_ROWS: usize = 1024;
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Time {
    before_epoch: bool,
    seconds: u64,
    nanos: u32,
}
impl Time {
    fn capture(time: std::time::SystemTime) -> Self {
        let (before_epoch, duration) = match time.duration_since(std::time::UNIX_EPOCH) {
            Ok(value) => (false, value),
            Err(value) => (true, value.duration()),
        };
        Self {
            before_epoch,
            seconds: duration.as_secs(),
            nanos: duration.subsec_nanos(),
        }
    }
    fn restore(self) -> io::Result<std::time::SystemTime> {
        if self.nanos >= 1_000_000_000 || self.before_epoch && self.seconds == 0 && self.nanos == 0
        {
            return Err(native::invalid("noncanonical native cache time"));
        }
        let duration = std::time::Duration::new(self.seconds, self.nanos);
        (if self.before_epoch {
            std::time::UNIX_EPOCH.checked_sub(duration)
        } else {
            std::time::UNIX_EPOCH.checked_add(duration)
        })
        .ok_or_else(|| native::invalid("native cache time range"))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Tile {
    path: Vec<u8>,
    #[serde(deserialize_with = "required")]
    version: Option<(u64, Time)>,
    #[serde(deserialize_with = "required")]
    image: Option<Id>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Placement {
    image: Id,
    area: (u16, u16, u16, u16),
    first_row: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    schema: u8,
    protocol: u8,
    cell_size: (u32, u32),
    background: (u8, u8, u8),
    cache: Vec<Tile>,
    placements: Vec<Placement>,
    rendered: Vec<RenderKey>,
    render_fault: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RenderKey {
    path: Vec<u8>,
    placeholder: String,
    #[serde(deserialize_with = "required")]
    version: Option<(u64, Time)>,
}
#[derive(Clone)]
struct ObservedKey {
    path: PathBuf,
    placeholder: String,
    version: Option<(u64, std::time::SystemTime)>,
}
impl ObservedKey {
    fn key(&self) -> TileKey {
        TileKey {
            path: self.path.clone(),
            version: self.version,
        }
    }
    fn data(&self) -> RenderKey {
        RenderKey {
            path: self.path.as_os_str().as_bytes().to_vec(),
            placeholder: self.placeholder.clone(),
            version: self.version.map(|(size, time)| (size, Time::capture(time))),
        }
    }
}
#[derive(Default)]
struct RenderProgress {
    current: Vec<ObservedKey>,
    replay: Option<Arc<Vec<ObservedKey>>>,
    cursor: usize,
    faulted: bool,
}
#[derive(Default)]
pub(super) struct RenderState {
    state: Arc<Mutex<RenderProgress>>,
    watch: Option<crate::retained_image_renderer::RestoreWatch>,
}
pub(super) struct RenderGuard<'a> {
    actual: &'a RenderState,
    plan: Option<Arc<Vec<ObservedKey>>>,
}
impl RenderState {
    fn restoring(
        data: Vec<RenderKey>,
        faulted: bool,
        watch: Option<crate::retained_image_renderer::RestoreWatch>,
    ) -> io::Result<Self> {
        if data.len() > MAX_ROWS {
            return Err(native::invalid("native rendered key capacity"));
        }
        let replay = data
            .into_iter()
            .map(|key| {
                if key.path.len() > 64 * 1024 || key.path.contains(&0) {
                    return Err(native::invalid("native rendered key path range"));
                }
                Ok(ObservedKey {
                    path: PathBuf::from(std::ffi::OsString::from_vec(key.path)),
                    placeholder: key.placeholder,
                    version: key
                        .version
                        .map(|(size, time)| time.restore().map(|time| (size, time)))
                        .transpose()?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let result = Self {
            state: Arc::new(Mutex::new(RenderProgress {
                replay: Some(Arc::new(replay)),
                faulted,
                ..RenderProgress::default()
            })),
            watch,
        };
        if faulted && let Some(watch) = &result.watch {
            watch.invalidate();
        }
        Ok(result)
    }
    fn fault(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.faulted = true;
        }
        if let Some(watch) = &self.watch {
            watch.invalidate();
        }
    }
    pub(super) fn begin(
        &self,
        area: Rect,
        attachments: &[LocalImageAttachment],
        textarea: &TextArea,
    ) -> Option<RenderGuard<'_>> {
        let plan = {
            let state = match self.state.lock() {
                Ok(state) => state,
                Err(error) => {
                    drop(error);
                    self.fault();
                    return None;
                }
            };
            if state.faulted {
                drop(state);
                self.fault();
                return None;
            }
            state.replay.clone()
        };
        if let Some(plan) = &plan {
            let mut ordered: Vec<_> = attachments.iter().collect();
            if ordered.len() > 1 {
                let positions: HashMap<_, _> = textarea
                    .text_element_ranges()
                    .map(|range| (&textarea.text()[range.clone()], range.start))
                    .collect();
                ordered.sort_by_key(|item| {
                    positions
                        .get(item.placeholder.as_str())
                        .copied()
                        .unwrap_or(usize::MAX)
                });
            }
            let visible = if area.is_empty() {
                0
            } else {
                visible_thumbnails(area.width, attachments.len()).0
            };
            let actual = ordered.into_iter().take(visible).collect::<Vec<_>>();
            if actual.len() != plan.len()
                || actual
                    .iter()
                    .zip(plan.iter())
                    .any(|(item, key)| item.path != key.path || item.placeholder != key.placeholder)
            {
                self.fault();
                return None;
            }
        }
        {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(error) => {
                    drop(error);
                    self.fault();
                    return None;
                }
            };
            state.current.clear();
            state.cursor = 0;
        }
        Some(RenderGuard { actual: self, plan })
    }
    pub(super) fn key(&self, attachment: &LocalImageAttachment) -> Option<TileKey> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(error) => {
                drop(error);
                self.fault();
                return None;
            }
        };
        let observed = if let Some(plan) = &state.replay {
            let Some(key) = plan.get(state.cursor).cloned() else {
                drop(state);
                self.fault();
                return None;
            };
            if key.path != attachment.path || key.placeholder != attachment.placeholder {
                drop(state);
                self.fault();
                return None;
            }
            key
        } else {
            ObservedKey {
                path: attachment.path.clone(),
                placeholder: attachment.placeholder.clone(),
                version: attachment
                    .path
                    .metadata()
                    .ok()
                    .and_then(|m| Some((m.len(), m.modified().ok()?))),
            }
        };
        state.cursor += 1;
        let key = observed.key();
        state.current.push(observed);
        Some(key)
    }
    fn capture(&self) -> io::Result<(Vec<RenderKey>, bool)> {
        let state = self.state.lock().map_err(native::invalid)?;
        let keys: &[ObservedKey] = state
            .replay
            .as_ref()
            .map_or(state.current.as_slice(), |keys| keys.as_slice());
        Ok((keys.iter().map(ObservedKey::data).collect(), state.faulted))
    }
}
impl Drop for RenderGuard<'_> {
    fn drop(&mut self) {
        let Some(plan) = &self.plan else {
            return;
        };
        let complete = self
            .actual
            .state
            .lock()
            .map(|state| !state.faulted && state.cursor == plan.len())
            .unwrap_or(false);
        if std::thread::panicking() || !complete {
            self.actual.fault();
            return;
        }
        let state = self.actual.state.clone();
        let plan = plan.clone();
        let watch = self.actual.watch.clone();
        // The plan remains authoritative through partial/error/unwind. Its
        // exact same Arc retires only at the real Tui outer-frame commit seam.
        let _registered = crate::retained_tui_images::defer_resource_commit(move || {
            let mut state = match state.lock() {
                Ok(state) => state,
                Err(error) => {
                    drop(error);
                    if let Some(watch) = watch {
                        watch.invalidate();
                    }
                    return Ok(());
                }
            };
            if state.faulted {
                if let Some(watch) = watch {
                    watch.invalidate();
                }
                return Ok(());
            }
            if state
                .replay
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &plan))
            {
                state.replay = None;
                if let Some(watch) = &watch {
                    watch.changed();
                }
            }
            Ok(())
        });
    }
}
impl Data {
    pub(crate) fn capture(
        actual: &ComposerImages,
        shared: &mut native::Capture,
    ) -> io::Result<Self> {
        let cache = actual.cache.try_borrow().map_err(native::invalid)?;
        let placements = actual.placements.try_borrow().map_err(native::invalid)?;
        if cache.len() > MAX_ROWS || placements.len() > MAX_ROWS {
            return Err(native::invalid("native image-cache capacity"));
        }
        let cache = cache
            .iter()
            .map(|(key, image)| {
                Ok(Tile {
                    path: key.path.as_os_str().as_bytes().to_vec(),
                    version: key.version.map(|(size, time)| (size, Time::capture(time))),
                    image: image
                        .as_ref()
                        .map(|image| shared.image(image))
                        .transpose()?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let placements = placements
            .iter()
            .map(|item| {
                if item.file.is_some() {
                    return Err(native::invalid(
                        "composer contains an unexpected terminal unlink owner",
                    ));
                }
                validate_placement(&item.image, item.area, item.first_row)?;
                Ok(Placement {
                    image: shared.image(&item.image)?,
                    area: (item.area.x, item.area.y, item.area.width, item.area.height),
                    first_row: item.first_row,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let (rendered, render_fault) = actual.alab_retained.capture()?;
        Ok(Self {
            schema: 2,
            rendered,
            render_fault,
            protocol: match actual.protocol {
                ImageProtocol::Kitty => 0,
                ImageProtocol::KittyLocalFile => 1,
                ImageProtocol::Sixel => 2,
            },
            cell_size: actual.cell_size,
            background: actual.background,
            cache,
            placements,
        })
    }
    #[cfg(test)]
    fn restore_data(self, shared: &native::Restored) -> io::Result<ComposerImages> {
        self.restore_bound_data(shared, None)
    }
    fn restore_bound_data(
        self,
        shared: &native::Restored,
        watch: Option<crate::retained_image_renderer::RestoreWatch>,
    ) -> io::Result<ComposerImages> {
        if self.schema != 2
            || self.cell_size.0 == 0
            || self.cell_size.1 == 0
            || self.cache.len() > MAX_ROWS
            || self.placements.len() > MAX_ROWS
        {
            return Err(native::invalid(
                "native image-cache schema/geometry/capacity",
            ));
        }
        let protocol = match self.protocol {
            0 => ImageProtocol::Kitty,
            1 => ImageProtocol::KittyLocalFile,
            2 => ImageProtocol::Sixel,
            _ => return Err(native::invalid("native cache protocol")),
        };
        let mut cache = HashMap::new();
        for tile in self.cache {
            if tile.path.contains(&0) || tile.path.len() > 64 * 1024 {
                return Err(native::invalid("native image path DATA range"));
            }
            let key = TileKey {
                path: PathBuf::from(std::ffi::OsString::from_vec(tile.path)),
                version: tile
                    .version
                    .map(|(size, time)| time.restore().map(|time| (size, time)))
                    .transpose()?,
            };
            let image = tile.image.map(|image| shared.image(image)).transpose()?;
            if cache.insert(key, image).is_some() {
                return Err(native::invalid("duplicate original native tile key"));
            }
        }
        let placements = self
            .placements
            .into_iter()
            .map(|item| {
                let image = shared.image(item.image)?;
                let (x, y, width, height) = item.area;
                let area = Rect::new(x, y, width, height);
                validate_placement(&image, area, item.first_row)?;
                Ok(ImagePlacement {
                    image,
                    area,
                    first_row: item.first_row,
                    file: None,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(ComposerImages {
            protocol,
            cell_size: self.cell_size,
            background: self.background,
            cache: RefCell::new(cache),
            placements: RefCell::new(placements),
            alab_retained: RenderState::restoring(self.rendered, self.render_fault, watch)?,
        })
    }
    pub(crate) fn restore(
        self,
        shared: &native::Restored,
        renderer: &crate::retained_image_renderer::Binding,
    ) -> io::Result<ComposerImages> {
        renderer.check()?;
        self.restore_bound_data(shared, Some(renderer.restore_watch()))
    }
}
fn validate_placement(image: &TranscriptImage, area: Rect, first: usize) -> io::Result<()> {
    let first = u32::try_from(first).map_err(native::invalid)?;
    let bottom = first
        .checked_add(u32::from(area.height))
        .ok_or_else(|| native::invalid("native image row overflow"))?;
    let multiplier = if image.protocol == ImageProtocol::Sixel {
        image.cell_height
    } else {
        image.pixels.height()
    };
    if image.rows == 0
        || first.checked_mul(multiplier).is_none()
        || bottom.checked_mul(multiplier).is_none()
    {
        return Err(native::invalid("native image crop overflow"));
    }
    Ok(())
}
#[cfg(test)]
#[path = "retained_composer_images_tests.rs"]
mod tests;
