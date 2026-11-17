// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Ordered LOCAL records from the actual writer, not a path/ID read fallback.
//! Deferred observes its real pending buffer. Materialized lends its actual
//! open file for bounded positional reads while the writer task owns the read.
use super::IoError;
#[cfg(unix)]
use super::RolloutCmd;
use super::RolloutItem;
use super::RolloutRecorder;
#[cfg(unix)]
use super::RolloutWriterState;
use super::SessionMeta;
#[cfg(unix)]
use tokio::sync::oneshot;

#[derive(Clone, Debug)]
pub struct RetainedLocalRollout {
    pub session_meta: SessionMeta,
    /// LOCAL canonical records only. A paginated virtual inherited prefix is
    /// not invented or silently claimed by this writer-local observation.
    pub items: Vec<RolloutItem>,
}
impl RolloutRecorder {
    pub async fn retained_local_rollout(
        &self,
        max_bytes: usize,
    ) -> std::io::Result<RetainedLocalRollout> {
        #[cfg(unix)]
        {
            let (ack, done) = oneshot::channel();
            let (stop, keep_running) = oneshot::channel();
            self.tx
                .send(RolloutCmd::ReadRetainedLocalRollout {
                    ack,
                    stop,
                    max_bytes,
                })
                .await
                .map_err(|error| {
                    self.writer_task.terminal_failure().unwrap_or_else(|| {
                        IoError::other(format!("actual local writer unavailable: {error}"))
                    })
                })?;
            let result = done.await.map_err(|error| {
                self.writer_task.terminal_failure().unwrap_or_else(|| {
                    IoError::other(format!("actual local writer read unavailable: {error}"))
                })
            })?;
            drop(keep_running);
            result
        }
        #[cfg(not(unix))]
        {
            let _ = max_bytes;
            Err(IoError::new(
                std::io::ErrorKind::Unsupported,
                "positional retained writer read unavailable",
            ))
        }
    }
}

#[cfg(unix)]
pub(super) async fn snapshot(
    state: &mut RolloutWriterState,
    max_bytes: usize,
    stop: oneshot::Sender<()>,
) -> std::io::Result<RetainedLocalRollout> {
    if max_bytes == 0 {
        return Err(IoError::new(
            std::io::ErrorKind::InvalidInput,
            "actual local writer read needs a bounded budget",
        ));
    }
    if stop.is_closed() {
        return Err(IoError::new(
            std::io::ErrorKind::Interrupted,
            "local writer observation cancelled",
        ));
    }
    if let Some(error) = &state.last_logged_error {
        return Err(IoError::other(format!(
            "actual local writer has unresolved failure: {error}"
        )));
    }
    if state.is_deferred() {
        let session_meta = state
            .meta
            .clone()
            .ok_or_else(|| IoError::other("actual deferred writer has no creation metadata"))?;
        check_budget(&session_meta, &state.pending_items, max_bytes)?;
        return Ok(RetainedLocalRollout {
            session_meta,
            items: state.pending_items.clone(),
        });
    }
    if !state.pending_items.is_empty() {
        return Err(IoError::other(
            "actual materialized writer has unobserved pending records",
        ));
    }
    let writer = state
        .writer
        .as_ref()
        .ok_or_else(|| IoError::other("actual materialized writer is unavailable"))?;
    let file = writer.file.try_clone().await?;
    let end = file.metadata().await?.len();
    if end > u64::try_from(max_bytes).map_err(IoError::other)? {
        return Err(IoError::new(
            std::io::ErrorKind::WouldBlock,
            "actual local writer snapshot exceeds response budget",
        ));
    }
    let file = file.into_std().await;
    // This SAME original writer task joins the actual reader before processing
    // any later add/shutdown command. Losing the response closes stop only.
    tokio::task::spawn_blocking(move || read(file, end, stop))
        .await
        .map_err(IoError::other)?
}

#[cfg(unix)]
fn check_budget(
    meta: &SessionMeta,
    items: &[RolloutItem],
    max_bytes: usize,
) -> std::io::Result<()> {
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.checked_sub(bytes.len()).ok_or_else(|| {
                IoError::new(
                    std::io::ErrorKind::WouldBlock,
                    "actual deferred writer snapshot exceeds response budget",
                )
            })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Budget(max_bytes), &(meta, items)).map_err(|error| {
        IoError::new(
            error
                .io_error_kind()
                .unwrap_or(std::io::ErrorKind::InvalidData),
            error,
        )
    })
}

#[cfg(unix)]
fn read(
    file: std::fs::File,
    end: u64,
    stop: oneshot::Sender<()>,
) -> std::io::Result<RetainedLocalRollout> {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(Positional {
        file,
        position: 0,
        end,
        stop,
    });
    let mut line = Vec::new();
    let mut metadata = None;
    let mut items = Vec::new();
    while reader.read_until(b'\n', &mut line)? != 0 {
        if !line.ends_with(b"\n") {
            return Err(IoError::other(
                "actual writer cutoff contains an incomplete record",
            ));
        }
        let record = crate::parse_rollout_line_bytes(&line).map_err(IoError::other)?;
        match record.item {
            RolloutItem::SessionMeta(meta) if metadata.is_none() && items.is_empty() => {
                metadata = Some(meta.meta)
            }
            RolloutItem::SessionMeta(_) => {
                return Err(IoError::other(
                    "actual writer has ambiguous creation metadata",
                ));
            }
            item if metadata.is_some() => items.push(item),
            _ => {
                return Err(IoError::other(
                    "actual writer record precedes creation metadata",
                ));
            }
        }
        line.clear();
    }
    let session_meta = metadata.ok_or_else(|| IoError::other("actual writer header is missing"))?;
    Ok(RetainedLocalRollout {
        session_meta,
        items,
    })
}

/// FileExt::read_at does not move the shared open-file-description cursor.
/// Cloning the writer FD and using ordinary seek/read would corrupt writes.
#[cfg(unix)]
struct Positional {
    file: std::fs::File,
    position: u64,
    end: u64,
    stop: oneshot::Sender<()>,
}
#[cfg(unix)]
impl std::io::Read for Positional {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        use std::os::unix::fs::FileExt;
        if self.stop.is_closed() {
            return Err(IoError::new(
                std::io::ErrorKind::Interrupted,
                "local writer observation cancelled",
            ));
        }
        let remaining = self
            .end
            .checked_sub(self.position)
            .ok_or_else(|| IoError::other("actual writer read exceeded its cutoff"))?;
        let limit = bytes
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        if limit == 0 {
            return Ok(0);
        }
        let read = self.file.read_at(&mut bytes[..limit], self.position)?;
        if read == 0 {
            return Err(IoError::new(
                std::io::ErrorKind::UnexpectedEof,
                "actual writer ended before its captured cutoff",
            ));
        }
        self.position = self
            .position
            .checked_add(u64::try_from(read).map_err(IoError::other)?)
            .ok_or_else(|| IoError::other("actual writer read position overflow"))?;
        Ok(read)
    }
}

#[cfg(all(test, unix))]
mod positional_tests {
    use super::*;
    use std::io::Read;
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;

    // SAME open-file description: ordinary seek on a cloned FD would change
    // the real writer's next cursor. This test uses actual OS descriptors.
    #[test]
    fn positional_cut_never_moves_the_actual_shared_writer_cursor()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut actual = tempfile::tempfile()?;
        actual.write_all(b"actual-prefix-and-later")?;
        actual.seek(SeekFrom::Start(7))?;
        let cloned = actual.try_clone()?;
        let (stop, keep_running) = oneshot::channel();
        let mut reader = Positional {
            file: cloned,
            position: 0,
            end: 13,
            stop,
        };
        let mut captured = Vec::new();
        reader.read_to_end(&mut captured)?;
        assert_eq!(captured, b"actual-prefix");
        assert_eq!(actual.stream_position()?, 7);
        drop(keep_running);
        let mut byte = [0];
        assert_eq!(
            reader.read(&mut byte).err().map(|e| e.kind()),
            Some(std::io::ErrorKind::Interrupted)
        );
        assert_eq!(actual.stream_position()?, 7);
        Ok(())
    }
}
