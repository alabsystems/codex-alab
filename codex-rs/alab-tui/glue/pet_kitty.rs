// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(pet-perf) glue, compiled inside codex-tui as `pets::alab_kitty` (one `#[path]` line in
//! `tui/src/pets/mod.rs`): the ambient pet inside aterm sends each frame's picture once and
//! then only places it by id. `codex_alab_tui::pets::kitty` says why, and when a frame is sent
//! again.
//!
//! Three hooks reach it: `render_ambient_pet_image` offers each draw to [`render`] first;
//! `PetImageRenderState` carries a [`Stored`]; `Tui::clear_ambient_pet_image` calls
//! [`PetImageRenderState::alab_release`], so an explicit clear (a pet switched off, the exit)
//! frees what aterm stored, while a pet a popup hides keeps it. Outside the codex binary inside
//! aterm, and for any draw that is not a kitty one of the pet's own frames, [`render`] hands the
//! draw back untouched: upstream's path, and upstream's tests, are unchanged.

use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose;
use codex_alab_tui::pets::kitty;
use crossterm::cursor::MoveTo;
use crossterm::cursor::RestorePosition;
use crossterm::cursor::SavePosition;
use crossterm::queue;

use super::AmbientPetDraw;
use super::PetImageRenderError;
use super::PetImageRenderState;
use super::image_protocol::ImageProtocol;

const ESC: &str = "\x1b";
const ST: &str = "\x1b\\";
/// Upstream's chunk size for a direct-medium transmit.
const CHUNK: usize = 4096;
/// The pet's one placement, moved from frame to frame.
const PLACEMENT: u32 = 1;
/// The frames aterm keeps at once. A frame there costs aterm its decoded raster and the copy
/// laid out for the pet's cells (a few hundred KB for a 192×208 frame). Keep two complete
/// ALab animation rows, so idle and running both stay stored, and the
/// frame drawn longest ago goes first.
const KEPT: usize = 2 * codex_alab_tui::pets::pack::COLUMNS;

#[cfg(test)]
thread_local! {
    static RELEASE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CLEAR_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What aterm holds for the ambient pet.
#[derive(Debug, Default)]
pub(super) struct Stored {
    /// The frames sent, the one drawn longest ago first: id and the file sent under it.
    sent: VecDeque<(u32, PathBuf)>,
    /// The [`kitty::epoch`] they were sent in.
    epoch: u64,
    /// The frame whose placement is on screen.
    placed: Option<u32>,
    /// The next clear frees the stored frames too.
    release: bool,
}

impl PetImageRenderState {
    /// The next clear is the pet's end, not a popup over it: free what aterm stored.
    pub(crate) fn alab_release(&mut self) {
        self.alab.release = true;
        #[cfg(test)]
        RELEASE_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    #[cfg(test)]
    pub(crate) fn alab_release_calls_for_test() -> usize {
        RELEASE_CALLS.with(std::cell::Cell::get)
    }

    #[cfg(test)]
    pub(crate) fn alab_clear_calls_for_test() -> usize {
        CLEAR_CALLS.with(std::cell::Cell::get)
    }
}

/// Offer one ambient pet draw (`None`: take it down) to the stored-frame path. `Ok(None)`:
/// drawn here. `Ok(Some(request))`: upstream's path draws it (after this path took down its
/// own placement, for a `None`).
pub(super) fn render(
    writer: &mut impl Write,
    state: &mut PetImageRenderState,
    request: Option<AmbientPetDraw>,
) -> Result<Option<Option<AmbientPetDraw>>, PetImageRenderError> {
    #[cfg(test)]
    if request.is_none() {
        CLEAR_CALLS.with(|calls| calls.set(calls.get() + 1));
    }
    render_when(
        kitty::on(),
        kitty::epoch(),
        writer,
        &mut state.alab,
        request,
    )
}

fn render_when(
    on: bool,
    epoch: u64,
    writer: &mut impl Write,
    stored: &mut Stored,
    request: Option<AmbientPetDraw>,
) -> Result<Option<Option<AmbientPetDraw>>, PetImageRenderError> {
    if !on {
        return Ok(Some(request));
    }
    if stored.epoch != epoch {
        // Whatever aterm held may be gone. What it still holds goes too, so it never holds
        // more than `KEPT`, and each frame is sent again as it next comes up.
        if !stored.sent.is_empty() || stored.placed.is_some() {
            write!(writer, "{}", delete_all())?;
        }
        stored.sent.clear();
        stored.placed = None;
        stored.epoch = epoch;
    }
    let Some(draw) = request else {
        if let Some(placed) = stored.placed.take() {
            write!(writer, "{}", delete_placement(placed))?;
        }
        if std::mem::take(&mut stored.release) && !stored.sent.is_empty() {
            write!(writer, "{}", delete_all())?;
            stored.sent.clear();
        }
        return Ok(Some(None));
    };
    let Some(id) = (draw.protocol == ImageProtocol::Kitty)
        .then(|| frame_index(&draw.frame))
        .flatten()
        .and_then(kitty::frame_id)
    else {
        return Ok(Some(Some(draw)));
    };

    // Another pet's frame under the same id is another picture.
    let held_at = stored.sent.iter().position(|(sent, _)| *sent == id);
    let held = held_at.is_some_and(|at| stored.sent[at].1 == draw.frame);
    if !held {
        let png = read_frame_png(&draw.frame)
            .map_err(|err| {
                anyhow::Error::new(err).context(format!("read {}", draw.frame.display()))
            })
            .map_err(PetImageRenderError::Asset)?;
        write!(writer, "{}", transmit(id, &png))?;
    }
    // A failed replacement must leave the previous image tracked for cleanup.
    if let Some(at) = held_at {
        let _ = stored.sent.remove(at);
    }
    stored.sent.push_back((id, draw.frame.clone()));
    while stored.sent.len() > KEPT {
        if let Some((oldest, _)) = stored.sent.pop_front() {
            write!(writer, "{}", free(oldest))?;
        }
    }
    if let Some(previous) = stored.placed.filter(|previous| *previous != id) {
        write!(writer, "{}", delete_placement(previous))?;
    }
    queue!(writer, SavePosition, MoveTo(draw.x, draw.y))?;
    write!(writer, "{}", place(id, draw.columns, draw.rows))?;
    queue!(writer, RestorePosition)?;
    writer.flush()?;
    stored.placed = Some(id);
    Ok(None)
}

/// The sprite index of one of `prepare_png_frames`' files (`frame_<index>.png`).
fn frame_index(frame: &Path) -> Option<usize> {
    frame
        .file_name()?
        .to_str()?
        .strip_prefix("frame_")?
        .strip_suffix(".png")?
        .parse()
        .ok()
}

fn read_frame_png(path: &Path) -> io::Result<Vec<u8>> {
    use std::io::Read as _;
    const MAX_FRAME_PNG_BYTES: u64 = 16 * 1024 * 1024;
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)?
    };
    #[cfg(not(unix))]
    let file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    if meta.len() > MAX_FRAME_PNG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame PNG exceeds 16 MiB limit",
        ));
    }
    let mut png = Vec::new();
    file.take(MAX_FRAME_PNG_BYTES + 1).read_to_end(&mut png)?;
    if png.len() as u64 > MAX_FRAME_PNG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame PNG exceeds 16 MiB limit",
        ));
    }
    Ok(png)
}

/// Store `png` under `id`, shown nowhere (`a=t`), in upstream's chunks.
fn transmit(id: u32, png: &[u8]) -> String {
    let payload = general_purpose::STANDARD.encode(png);
    let chunks = payload.as_bytes().chunks(CHUNK).collect::<Vec<_>>();
    let mut command = String::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let more = u8::from(index + 1 < chunks.len());
        let chunk = String::from_utf8_lossy(chunk);
        if index == 0 {
            command.push_str(&format!(
                "{ESC}_Ga=t,t=d,f=100,i={id},q=2,m={more};{chunk}{ST}"
            ));
        } else {
            command.push_str(&format!("{ESC}_Gm={more};{chunk}{ST}"));
        }
    }
    command
}

/// Show the stored `id` at the cursor as the pet's one placement, `columns` by `rows` cells,
/// leaving the cursor where it is. `q=1` asks aterm to answer only a failure, which
/// `glue/kitty_replies.rs` takes off the input; on Windows nothing reads it there, so nothing
/// is asked.
fn place(id: u32, columns: u16, rows: u16) -> String {
    let quiet = if cfg!(windows) { 2 } else { 1 };
    format!("{ESC}_Ga=p,i={id},p={PLACEMENT},c={columns},r={rows},C=1,q={quiet};{ST}")
}

/// Take the placement of `id` down and keep its data.
fn delete_placement(id: u32) -> String {
    format!("{ESC}_Ga=d,d=i,i={id},p={PLACEMENT},q=2;{ST}")
}

/// Free `id`'s data.
fn free(id: u32) -> String {
    format!("{ESC}_Ga=d,d=I,i={id},q=2;{ST}")
}

/// Free every frame id the pet has, and its placement.
fn delete_all() -> String {
    let (first, last) = (kitty::FRAME_IDS.start(), kitty::FRAME_IDS.end());
    format!("{ESC}_Ga=d,d=R,x={first},y={last},q=2;{ST}")
}

/// Keep a valid local filename whose encoded path contains the image fixture's
/// `cG5n` bytes, independent of the temporary directory's byte alignment.
#[cfg(test)]
pub(super) fn local_file_frame_for_test(directory: &Path) -> PathBuf {
    let directory = directory.canonicalize().expect("canonical test directory");
    (0..3)
        .map(|padding| directory.join(format!("{}png.png", "x".repeat(padding))))
        .find(|path| {
            general_purpose::STANDARD
                .encode(path.to_string_lossy().as_bytes())
                .contains("cG5n")
        })
        .expect("one of the three filename byte alignments contains encoded png")
}

/// A file reference carries the exact canonical path, not the image bytes.
#[cfg(test)]
pub(super) fn assert_local_file_reference_for_test(output: &str, directory: &Path) {
    // Other protocol tests may set TMUX; its passthrough doubles escape bytes
    // in graphics commands without changing their control keys or payload.
    let output = output.replace("\x1b\x1b", "\x1b");
    let graphics: Vec<_> = output
        .split("\x1b_G")
        .skip(1)
        .map(|command| command.split_once(ST).expect("graphics terminator").0)
        .collect();
    assert_eq!(graphics.len(), 2, "one deletion and one transmission");
    let deletions: Vec<_> = graphics
        .iter()
        .filter_map(|command| command.split_once(';'))
        .filter(|(control, _)| control.starts_with("a=d,"))
        .collect();
    assert_eq!(deletions.len(), 1, "exactly one image deletion");
    assert!(deletions[0].1.is_empty(), "deletion payload must be empty");
    let references: Vec<_> = graphics
        .iter()
        .filter_map(|command| command.split_once(';'))
        .filter(|(control, _)| control.starts_with("a=T,"))
        .collect();
    assert_eq!(references.len(), 1, "exactly one image transmission");
    let (control, payload) = references[0];
    assert!(control.split(',').any(|key| key == "t=f"));
    let decoded = general_purpose::STANDARD
        .decode(payload)
        .expect("base64 filename");
    let expected = local_file_frame_for_test(directory);
    assert_eq!(decoded, expected.to_string_lossy().as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    struct Frames {
        _dir: tempfile::TempDir,
        paths: Vec<PathBuf>,
    }

    fn frames(count: usize) -> Frames {
        let dir = tempfile::tempdir().unwrap();
        let paths = (0..count)
            .map(|index| {
                let path = dir.path().join(format!("frame_{index:03}.png"));
                std::fs::write(&path, format!("png{index}")).unwrap();
                path
            })
            .collect();
        Frames { _dir: dir, paths }
    }

    fn draw(frame: &Path) -> AmbientPetDraw {
        AmbientPetDraw {
            frame: frame.to_path_buf(),
            protocol: ImageProtocol::Kitty,
            x: 2,
            y: 3,
            clear_top_y: 3,
            columns: 4,
            rows: 5,
            height_px: 75,
            sixel_dir: PathBuf::new(),
        }
    }

    fn drawn(stored: &mut Stored, epoch: u64, request: Option<AmbientPetDraw>) -> String {
        let mut output = Vec::new();
        render_when(/*on*/ true, epoch, &mut output, stored, request).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn payload(index: usize) -> String {
        general_purpose::STANDARD.encode(format!("png{index}"))
    }

    fn id(index: usize) -> u32 {
        kitty::frame_id(index).unwrap()
    }

    #[test]
    fn frame_reads_reject_non_regular_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_frame_png(dir.path()).is_err());
        let path = dir.path().join("frame.png");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(16 * 1024 * 1024 + 1).unwrap();
        assert_eq!(
            read_frame_png(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        std::fs::write(&path, b"png").unwrap();
        assert_eq!(read_frame_png(&path).unwrap(), b"png");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            let fifo_path = dir.path().join("frame-fifo");
            let fifo = std::ffi::CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
            // SAFETY: valid NUL-terminated path owned by this private fixture.
            assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
            assert_eq!(
                read_frame_png(&fifo_path).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn a_local_file_deletion_cannot_carry_image_payload() {
        let directory = tempfile::tempdir().unwrap();
        let frame = local_file_frame_for_test(directory.path());
        std::fs::write(&frame, b"png").unwrap();
        let mut request = draw(&frame);
        request.protocol = ImageProtocol::KittyLocalFile;
        let mut output = Vec::new();
        super::super::render_ambient_pet_image(
            &mut output,
            &mut PetImageRenderState::default(),
            Some(request),
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_local_file_reference_for_test(&output, directory.path());
        let corrupted = output.replacen("Ga=d,d=I,i=49374,q=2;", "Ga=d,d=I,i=49374,q=2;cG5n", 1);
        assert_ne!(corrupted, output, "fixture changes the actual deletion");
        let expected_directory = directory.path();
        let rejected = std::panic::catch_unwind(|| {
            assert_local_file_reference_for_test(&corrupted, expected_directory);
        });
        assert!(
            rejected.is_err(),
            "oracle accepted unexpected deletion data"
        );
    }

    #[test]
    fn an_unchanged_pet_sends_its_picture_once_then_only_places_it() {
        let frames = frames(1);
        let mut stored = Stored::default();

        let first = drawn(&mut stored, 0, Some(draw(&frames.paths[0])));
        assert!(first.contains(&format!("\x1b_Ga=t,t=d,f=100,i={},q=2,m=0;", id(0))));
        assert!(first.contains(&payload(0)));
        let quiet = if cfg!(windows) { 2 } else { 1 };
        let place = format!("\x1b_Ga=p,i={},p=1,c=4,r=5,C=1,q={quiet};\x1b\\", id(0));
        assert_eq!(first.matches(&place).count(), 1);

        let second = drawn(&mut stored, 0, Some(draw(&frames.paths[0])));
        assert!(!second.contains("a=t"), "{second:?}");
        assert!(!second.contains(&payload(0)), "{second:?}");
        assert!(
            !second.contains("a=d"),
            "the placement moves in place: {second:?}"
        );
        assert_eq!(second, format!("\x1b7\x1b[4;3H{place}\x1b8"));
    }

    #[test]
    fn a_cleared_screen_sends_the_picture_again_once() {
        let frames = frames(1);
        let mut stored = Stored::default();
        drawn(&mut stored, 0, Some(draw(&frames.paths[0])));
        drawn(&mut stored, 0, Some(draw(&frames.paths[0])));

        // A clear (or aterm's ENOENT) moves the epoch.
        let after_clear = drawn(&mut stored, 1, Some(draw(&frames.paths[0])));
        assert_eq!(after_clear.matches(&payload(0)).count(), 1);
        assert_eq!(after_clear.matches("a=t,").count(), 1);
        let first = id(0);
        assert!(
            after_clear.starts_with(&format!("\x1b_Ga=d,d=R,x={first},y=")),
            "what aterm still held goes first: {after_clear:?}"
        );

        let next = drawn(&mut stored, 1, Some(draw(&frames.paths[0])));
        assert!(!next.contains(&payload(0)), "{next:?}");
        assert!(!next.contains("a=t"), "{next:?}");
    }

    #[test]
    fn a_new_frame_moves_the_one_placement_and_old_frames_stay_stored() {
        let frames = frames(2);
        let mut stored = Stored::default();
        drawn(&mut stored, 0, Some(draw(&frames.paths[0])));

        let second = drawn(&mut stored, 0, Some(draw(&frames.paths[1])));
        assert!(second.contains(&payload(1)));
        let take_down = format!("\x1b_Ga=d,d=i,i={},p=1,q=2;\x1b\\", id(0));
        let place = format!("a=p,i={},p=1,", id(1));
        assert!(second.find(&take_down).unwrap() < second.find(&place).unwrap());

        // Back to the first frame: placed, not sent.
        let third = drawn(&mut stored, 0, Some(draw(&frames.paths[0])));
        assert!(!third.contains("a=t"), "{third:?}");
        assert!(third.contains(&format!("a=p,i={},p=1,", id(0))));
    }

    #[test]
    fn a_hidden_pet_keeps_its_frames_and_its_end_frees_them() {
        let frames = frames(1);
        let mut stored = Stored::default();
        drawn(&mut stored, 0, Some(draw(&frames.paths[0])));

        let hidden = drawn(&mut stored, 0, None);
        assert_eq!(hidden, format!("\x1b_Ga=d,d=i,i={},p=1,q=2;\x1b\\", id(0)));
        let shown = drawn(&mut stored, 0, Some(draw(&frames.paths[0])));
        assert!(!shown.contains("a=t"), "{shown:?}");

        stored.release = true;
        let released = drawn(&mut stored, 0, None);
        assert!(released.contains("a=d,d=i,"));
        assert!(released.contains(&format!("\x1b_Ga=d,d=R,x={},y=", id(0))));
        stored.release = true;
        assert!(drawn(&mut stored, 0, None).is_empty());
        let again = drawn(&mut stored, 0, Some(draw(&frames.paths[0])));
        assert!(again.contains(&payload(0)), "{again:?}");
    }

    #[test]
    fn aterm_keeps_only_the_frames_drawn_last() {
        let frames = frames(KEPT + 1);
        let mut stored = Stored::default();
        for path in &frames.paths[..KEPT] {
            drawn(&mut stored, 0, Some(draw(path)));
        }
        // Frame 0 drawn again is the newest; frame 1 is now the oldest.
        drawn(&mut stored, 0, Some(draw(&frames.paths[0])));
        let over = drawn(&mut stored, 0, Some(draw(&frames.paths[KEPT])));
        assert!(over.contains(&format!("\x1b_Ga=d,d=I,i={},q=2;\x1b\\", id(1))));
        assert_eq!(over.matches("d=I").count(), 1);
        let kept = drawn(&mut stored, 0, Some(draw(&frames.paths[0])));
        assert!(!kept.contains("a=t"), "{kept:?}");
    }

    #[test]
    fn idle_and_running_cycles_stay_cached_across_state_changes() {
        use codex_alab_tui::pets::pack;

        let frames = frames(pack::COLUMNS * pack::ROWS);
        let rows = ["idle", "running"]
            .map(|name| pack::ROW_NAMES.iter().position(|row| *row == name).unwrap());
        let mut stored = Stored::default();
        for row in rows {
            for column in 0..pack::COLUMNS {
                let index = row * pack::COLUMNS + column;
                let output = drawn(&mut stored, 0, Some(draw(&frames.paths[index])));
                assert_eq!(output.matches("a=t,").count(), 1);
            }
        }

        // An idle/working transition must not evict either complete animation.
        // Repeated warm cycles should only place frames already in the terminal.
        for row in rows.into_iter().cycle().take(6) {
            for column in 0..pack::COLUMNS {
                let index = row * pack::COLUMNS + column;
                let output = drawn(&mut stored, 0, Some(draw(&frames.paths[index])));
                assert!(!output.contains("a=t,"), "frame {index} was sent again");
                assert!(!output.contains("d=I,"), "a warm frame was evicted");
                assert!(output.contains(&format!("a=p,i={},p=1,", id(index))));
            }
        }
    }

    #[test]
    fn a_frame_file_sent_again_under_the_same_id_is_sent() {
        let first = frames(1);
        let second = frames(1);
        let mut stored = Stored::default();
        drawn(&mut stored, 0, Some(draw(&first.paths[0])));
        // Another pet's frame 0: same id, another picture.
        let other = drawn(&mut stored, 0, Some(draw(&second.paths[0])));
        assert!(other.contains("a=t,"), "{other:?}");
        assert_eq!(stored.sent.len(), 1);
    }

    #[test]
    fn a_missing_replacement_frame_keeps_the_old_image_tracked_for_cleanup() {
        let original = frames(1);
        let replacement = tempfile::tempdir().unwrap();
        let mut stored = Stored::default();
        drawn(&mut stored, 0, Some(draw(&original.paths[0])));

        let mut output = Vec::new();
        let error = render_when(
            true,
            0,
            &mut output,
            &mut stored,
            Some(draw(&replacement.path().join("frame_000.png"))),
        )
        .unwrap_err();
        assert!(matches!(error, PetImageRenderError::Asset(_)));
        assert!(output.is_empty());

        // The app disables the pet and releases its images after an asset error.
        stored.release = true;
        let released = drawn(&mut stored, 0, None);
        assert!(
            released.contains("a=d,d=R,"),
            "the failed replacement left the old image stored: {released:?}"
        );
        assert!(stored.sent.is_empty());
        assert!(stored.placed.is_none());
    }

    #[test]
    fn off_or_not_a_frame_is_upstreams_draw() {
        let frames = frames(1);
        let mut stored = Stored::default();
        let mut output = Vec::new();
        let back = render_when(
            /*on*/ false,
            0,
            &mut output,
            &mut stored,
            Some(draw(&frames.paths[0])),
        )
        .unwrap();
        assert!(output.is_empty());
        assert_eq!(
            back.flatten().map(|draw| draw.frame),
            Some(frames.paths[0].clone())
        );

        let mut sixel = draw(&frames.paths[0]);
        sixel.protocol = ImageProtocol::Sixel;
        let back = render_when(/*on*/ true, 0, &mut output, &mut stored, Some(sixel)).unwrap();
        assert!(output.is_empty());
        assert!(matches!(back, Some(Some(_))));

        let mut stranger = draw(&frames.paths[0]);
        stranger.frame = frames.paths[0].with_file_name("custom.png");
        let back = render_when(
            /*on*/ true,
            0,
            &mut output,
            &mut stored,
            Some(stranger),
        )
        .unwrap();
        assert!(output.is_empty());
        assert!(matches!(back, Some(Some(_))));
    }

    #[test]
    fn a_missing_frame_is_an_asset_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut stored = Stored::default();
        let mut output = Vec::new();
        let err = render_when(
            /*on*/ true,
            0,
            &mut output,
            &mut stored,
            Some(draw(&dir.path().join("frame_000.png"))),
        )
        .unwrap_err();
        assert!(matches!(err, PetImageRenderError::Asset(_)));
        assert!(stored.sent.is_empty());
    }

    #[test]
    fn a_clear_moves_the_epoch() {
        use crate::custom_terminal::Terminal;
        use crate::test_backend::VT100Backend;
        let backend = VT100Backend::new(/*width*/ 20, /*height*/ 6);
        let mut terminal = Terminal::with_options(backend).unwrap();
        let before = kitty::epoch();
        terminal.clear_visible_screen().unwrap();
        assert!(kitty::epoch() > before);
    }
}
