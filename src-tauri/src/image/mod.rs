//! Inline-image protocol plumbing (Kitty Graphics + SIXEL).
//!
//! This module owns the adapter between APC/DCS byte payloads and
//! `term_images::ImageProcessor` ([`parse`]) plus the event splitter that
//! separates protocol responses (echoed back to the PTY) from the
//! decode/display events the [`crate::viewer::image::ImageViewerRouter`]
//! consumes.
//!
//! Images are *not* rendered inline in the terminal grid — that matched
//! nothing the WebView build ever did. A `Place` event opens the native
//! image-viewer child window instead (`viewer/image.rs`).

pub mod parse;

use term_images::image_proc::ImageEvent;

/// Split a stream of [`ImageEvent`]s produced by [`parse::decode_apc`] /
/// [`parse::decode_dcs`] into:
/// 1. the events the image-viewer router should consume (ImageReady,
///    Place, Delete, Animation, QueryResponse),
/// 2. the response bytes the caller must echo back to the PTY (Kitty OK
///    replies, query responses, …).
pub fn split_image_events(events: Vec<ImageEvent>) -> (Vec<ImageEvent>, Vec<String>) {
    let mut state = Vec::with_capacity(events.len());
    let mut responses = Vec::new();
    for e in events {
        match e {
            ImageEvent::Response { data } => responses.push(data),
            other => state.push(other),
        }
    }
    (state, responses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_images::image_proc::ImageDelete;

    #[test]
    fn split_image_events_separates_responses() {
        let events = vec![
            ImageEvent::Response {
                data: "\x1b_Gi=1;OK\x1b\\".to_string(),
            },
            ImageEvent::Delete {
                target: ImageDelete::All,
            },
        ];
        let (state, resp) = split_image_events(events);
        assert_eq!(state.len(), 1);
        assert_eq!(resp.len(), 1);
        assert!(resp[0].contains("OK"));
    }
}
