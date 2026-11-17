//! Keep queued submissions paused until the transcript selector releases its owner.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

#[derive(Debug)]
pub(crate) struct CopyInputGuard(pub(crate) AppEventSender);

impl Drop for CopyInputGuard {
    fn drop(&mut self) {
        #[cfg(unix)] if !alab_retained_graph::release_event(&self.0) { return; } // ALAB(retained-ui): failed native graph restoration cannot publish a local close.
        self.0.send(AppEvent::TranscriptCopyClosed);
    }
}

#[cfg(unix)]
#[path = "../../alab-tui/glue/retained_copy_input_graph.rs"]
pub(crate) mod alab_retained_graph; // ALAB(retained-ui): original local strong/weak pause aliases bind to actual new widget senders.
