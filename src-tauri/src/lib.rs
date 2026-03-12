//! eMterm - Cross-platform terminal emulator with rich rendering capabilities.
//!
//! This is the main library for the Tauri backend, providing PTY functionality
//! and IPC commands for the frontend.

rust_i18n::i18n!("locales", fallback = "en");

#[cfg(feature = "gui")]
pub mod ansi;
#[cfg(feature = "gui")]
pub mod image;
#[cfg(feature = "gui")]
pub mod logging;
#[cfg(feature = "gui")]
pub mod pty;

// CLI command modules
pub mod commands;
pub mod encoding;
pub mod error;
pub mod protocols;
pub mod sftp;
pub mod ssh;
pub mod validation;

// GUI modules (split from lib.rs)
#[cfg(feature = "gui")]
mod app;
#[cfg(feature = "gui")]
pub mod download_registry;
#[cfg(feature = "gui")]
pub mod payloads;
#[cfg(feature = "gui")]
pub mod reader;
#[cfg(feature = "gui")]
pub mod state;
#[cfg(feature = "gui")]
pub mod tauri_commands;

#[cfg(all(feature = "gui", not(test)))]
pub use app::run;

// ============================================================================
// Tests
// ============================================================================

#[cfg(all(test, feature = "gui"))]
#[allow(deprecated)]
mod tests {
    use crate::image;
    use crate::payloads::ImageEventPayload;
    use crate::pty::PtyManager;
    use crate::state::{LARGE_IMAGE_DATA_THRESHOLD, LargeImageDataStore};
    use crate::{ansi, commands, protocols};

    #[tokio::test]
    async fn test_session_count_command() {
        let manager = PtyManager::new();

        // Initially, session count should be 0
        assert_eq!(manager.session_count().await, 0);

        // Create a session
        let session_id = manager.create_session(None, None, 80, 24).await.unwrap();
        assert_eq!(manager.session_count().await, 1);

        // Create another session
        let session_id2 = manager.create_session(None, None, 80, 24).await.unwrap();
        assert_eq!(manager.session_count().await, 2);

        // Remove one session
        if let Some(session) = manager.remove_session(&session_id).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
        assert_eq!(manager.session_count().await, 1);

        // Remove the other session
        if let Some(session) = manager.remove_session(&session_id2).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
        assert_eq!(manager.session_count().await, 0);
    }

    #[test]
    fn test_image_event_payload_serialization() {
        let payload = ImageEventPayload {
            session_id: "test-session-123".to_string(),
            event: image::ImageEvent::QueryResponse { supported: true },
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("test-session-123"));
        assert!(json.contains("QueryResponse"));
        assert!(json.contains("supported"));
    }

    #[test]
    fn test_image_event_payload_image_ready() {
        let decoded_image = image::DecodedImage {
            id: 42,
            width: 100,
            height: 50,
            rgba_data: vec![0; 20000],
            rgba_base64: "AAAA".to_string(),
        };

        let payload = ImageEventPayload {
            session_id: "session-456".to_string(),
            event: image::ImageEvent::ImageReady {
                image: decoded_image,
            },
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("session-456"));
        assert!(json.contains("ImageReady"));
        assert!(json.contains("\"id\":42"));
        assert!(json.contains("\"width\":100"));
        assert!(json.contains("\"height\":50"));
    }

    #[test]
    fn test_image_event_payload_place() {
        let placement = image::ImagePlacement {
            image_id: 1,
            placement_id: 2,
            row: 10,
            col: 20,
            columns: 80,
            rows: 24,
            x_offset: 0,
            y_offset: 0,
            z_index: -1,
        };

        let payload = ImageEventPayload {
            session_id: "session-789".to_string(),
            event: image::ImageEvent::Place { placement },
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("session-789"));
        assert!(json.contains("Place"));
        assert!(json.contains("\"image_id\":1"));
        assert!(json.contains("\"placement_id\":2"));
    }

    #[test]
    fn test_image_event_payload_delete() {
        let payload = ImageEventPayload {
            session_id: "session-delete".to_string(),
            event: image::ImageEvent::Delete {
                target: image::ImageDelete::All,
            },
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("session-delete"));
        assert!(json.contains("Delete"));
        assert!(json.contains("All"));
    }

    #[tokio::test]
    async fn test_large_image_data_store() {
        let store = LargeImageDataStore::new();

        // Store data
        {
            let mut data = store.data.lock().await;
            data.insert(
                ("session1".to_string(), 42),
                "large_base64_data".to_string(),
            );
        }

        // Retrieve data (one-shot: removes from store)
        {
            let mut data = store.data.lock().await;
            let result = data.remove(&("session1".to_string(), 42));
            assert_eq!(result, Some("large_base64_data".to_string()));
        }

        // Second retrieval should return None
        {
            let mut data = store.data.lock().await;
            let result = data.remove(&("session1".to_string(), 42));
            assert_eq!(result, None);
        }
    }

    #[test]
    fn test_large_image_data_threshold() {
        // Verify the threshold is reasonable (2MB base64 ≈ 1.5MB raw)
        assert_eq!(LARGE_IMAGE_DATA_THRESHOLD, 2_000_000);
    }

    /// End-to-end test for the batch Kitty chunk processing flow.
    ///
    /// Simulates the exact data flow: CLI generates Kitty sequence → WASM parser
    /// extracts APC bodies → frontend batches strings → backend processes via
    /// parse_kitty_command + ImageProcessor.
    #[test]
    fn test_kitty_batch_flow_end_to_end() {
        use ::image::{DynamicImage, RgbaImage};

        // Create a 100x100 test image (produces ~350 bytes PNG → 1 chunk)
        let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));

        // Step 1: CLI generates Kitty sequence
        let (sequence, _image_id) = protocols::kitty::generate_kitty_sequence(&img).unwrap();

        // Step 2: Extract APC bodies (simulating WASM parser)
        let apc_bodies = extract_apc_bodies(&sequence);
        assert!(!apc_bodies.is_empty(), "Should have at least one APC body");

        // Step 3: Process through batch path (simulating process_kitty_batch)
        let mut processor = image::ImageProcessor::new();
        let mut all_events: Vec<image::ImageEvent> = Vec::new();

        for body in &apc_bodies {
            if let Some(cmd) = ansi::apc::parse_kitty_command(body.as_bytes()) {
                let events = processor.process_kitty_command(&cmd, 0, 0);
                all_events.extend(events);
            }
        }

        // Step 4: Verify image was decoded successfully
        let has_image_ready = all_events
            .iter()
            .any(|e| matches!(e, image::ImageEvent::ImageReady { .. }));
        assert!(has_image_ready, "Should have ImageReady event");
    }

    /// End-to-end test for large multi-chunk Kitty batch processing.
    ///
    /// Uses a larger image that produces multiple APC chunks (~4096 bytes each).
    #[test]
    fn test_kitty_batch_flow_large_image() {
        use ::image::{DynamicImage, RgbaImage};

        // Create a 400x400 image (produces ~4KB+ PNG → multiple chunks)
        let img = DynamicImage::ImageRgba8(RgbaImage::new(400, 400));

        // CLI generates Kitty sequence
        let (sequence, _image_id) = protocols::kitty::generate_kitty_sequence(&img).unwrap();

        // Extract APC bodies
        let apc_bodies = extract_apc_bodies(&sequence);
        assert!(
            apc_bodies.len() > 1,
            "Large image should produce multiple chunks, got {}",
            apc_bodies.len()
        );

        // Process through batch path
        let mut processor = image::ImageProcessor::new();
        let mut all_events: Vec<image::ImageEvent> = Vec::new();

        for body in &apc_bodies {
            if let Some(cmd) = ansi::apc::parse_kitty_command(body.as_bytes()) {
                let events = processor.process_kitty_command(&cmd, 0, 0);
                all_events.extend(events);
            }
        }

        // Verify image was decoded successfully
        let image_ready = all_events.iter().find_map(|e| {
            if let image::ImageEvent::ImageReady { image } = e {
                Some(image)
            } else {
                None
            }
        });
        assert!(image_ready.is_some(), "Should have ImageReady event");
        let img = image_ready.unwrap();
        assert_eq!(img.width, 400);
        assert_eq!(img.height, 400);
    }

    /// Test batch flow with a very large image producing hundreds of chunks.
    /// This simulates the actual scenario: 1080x1920 image → ~2.4MB base64 → ~600 chunks.
    #[test]
    fn test_kitty_batch_flow_very_large_image() {
        use ::image::{DynamicImage, Rgba, RgbaImage};

        // Create a 1080x1920 image (matching the failing test case dimensions)
        // Fill with varied pixel data to prevent extreme compression
        let mut img = RgbaImage::new(1080, 1920);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
        let dyn_img = DynamicImage::ImageRgba8(img);

        // CLI generates Kitty sequence
        let (sequence, _image_id) = protocols::kitty::generate_kitty_sequence(&dyn_img).unwrap();

        // Extract APC bodies
        let apc_bodies = extract_apc_bodies(&sequence);
        assert!(
            apc_bodies.len() > 100,
            "Very large image should produce many chunks, got {}",
            apc_bodies.len()
        );

        // Process through batch path
        let mut processor = image::ImageProcessor::new();
        let mut all_events: Vec<image::ImageEvent> = Vec::new();

        for body in &apc_bodies {
            if let Some(cmd) = ansi::apc::parse_kitty_command(body.as_bytes()) {
                let events = processor.process_kitty_command(&cmd, 0, 0);
                all_events.extend(events);
            }
        }

        // Verify image was decoded successfully
        let image_ready = all_events.iter().find_map(|e| {
            if let image::ImageEvent::ImageReady { image } = e {
                Some(image)
            } else {
                None
            }
        });
        assert!(image_ready.is_some(), "Should have ImageReady event");
        let img = image_ready.unwrap();
        assert_eq!(img.width, 1080);
        assert_eq!(img.height, 1920);
        assert!(!img.rgba_base64.is_empty());
    }

    /// Test that simulates the full tmux DCS passthrough roundtrip.
    ///
    /// Flow: generate_kitty_sequence → wrap_each_sequence (tmux wrap)
    ///       → simulate_tmux_unwrap → extract_apc_bodies → process → verify
    #[test]
    fn test_tmux_passthrough_roundtrip_large_image() {
        use ::image::{DynamicImage, Rgba, RgbaImage};

        // Create a large image (400x400 → multiple chunks)
        let mut img = RgbaImage::new(400, 400);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]);
        }
        let dyn_img = DynamicImage::ImageRgba8(img);

        // Step 1: Generate Kitty sequence (same as CLI does)
        let (sequence, _image_id) = protocols::kitty::generate_kitty_sequence(&dyn_img).unwrap();

        // Extract original APC bodies (baseline)
        let original_bodies = extract_apc_bodies(&sequence);
        assert!(
            original_bodies.len() > 1,
            "Should produce multiple chunks, got {}",
            original_bodies.len()
        );

        // Step 2: Wrap for tmux (simulating passthrough_if_needed)
        let wrapped = commands::tmux::wrap_each_sequence_for_test(&sequence);

        // Verify the wrapped output is larger (DCS overhead + ESC doubling)
        assert!(wrapped.len() > sequence.len());

        // Step 3: Simulate tmux unwrapping
        let unwrapped = simulate_tmux_unwrap(&wrapped);

        // Step 4: The unwrapped data should be identical to the original
        assert_eq!(
            unwrapped, sequence,
            "Tmux roundtrip should preserve data exactly"
        );

        // Step 5: Extract APC bodies from unwrapped data
        let roundtrip_bodies = extract_apc_bodies(&unwrapped);
        assert_eq!(
            roundtrip_bodies.len(),
            original_bodies.len(),
            "Roundtrip should preserve chunk count"
        );
        for (i, (orig, rt)) in original_bodies
            .iter()
            .zip(roundtrip_bodies.iter())
            .enumerate()
        {
            assert_eq!(orig, rt, "Chunk {} differs after roundtrip", i);
        }

        // Step 6: Process through batch path → verify ImageReady
        let mut processor = image::ImageProcessor::new();
        let mut all_events: Vec<image::ImageEvent> = Vec::new();
        for body in &roundtrip_bodies {
            if let Some(cmd) = ansi::apc::parse_kitty_command(body.as_bytes()) {
                let events = processor.process_kitty_command(&cmd, 0, 0);
                all_events.extend(events);
            }
        }
        let image_ready = all_events.iter().find_map(|e| {
            if let image::ImageEvent::ImageReady { image } = e {
                Some(image)
            } else {
                None
            }
        });
        assert!(
            image_ready.is_some(),
            "Should have ImageReady after tmux roundtrip"
        );
        let decoded = image_ready.unwrap();
        assert_eq!(decoded.width, 400);
        assert_eq!(decoded.height, 400);
    }

    /// Test tmux roundtrip with frontend-style accumulation (single assembled chunk).
    ///
    /// Simulates: tmux unwrap → WASM parser extracts APC bodies →
    /// frontend accumulates base64 → sends single chunk → backend decodes.
    #[test]
    fn test_tmux_passthrough_with_frontend_accumulation() {
        use ::image::{DynamicImage, Rgba, RgbaImage};

        let mut img = RgbaImage::new(400, 400);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]);
        }
        let dyn_img = DynamicImage::ImageRgba8(img);

        let (sequence, _) = protocols::kitty::generate_kitty_sequence(&dyn_img).unwrap();
        let wrapped = commands::tmux::wrap_each_sequence_for_test(&sequence);
        let unwrapped = simulate_tmux_unwrap(&wrapped);
        let bodies = extract_apc_bodies(&unwrapped);
        assert!(bodies.len() > 1);

        // Simulate frontend accumulation (handleApcCallback logic)
        let mut first_chunk_body: Option<String> = None;
        let mut accumulated_payload = String::new();

        for body in &bodies {
            let semicolon_idx = body.find(';');
            let params = match semicolon_idx {
                Some(idx) => &body[..idx],
                None => body.as_str(),
            };
            let payload = match semicolon_idx {
                Some(idx) => &body[idx + 1..],
                None => "",
            };
            let is_more = params.contains("m=1");

            if is_more {
                if first_chunk_body.is_none() {
                    first_chunk_body = Some(body.clone());
                }
                accumulated_payload.push_str(payload);
            } else {
                // Final chunk
                accumulated_payload.push_str(payload);

                if let Some(ref first) = first_chunk_body {
                    let first_semi = first.find(';').unwrap_or(first.len());
                    let first_params = &first[..first_semi];
                    let fixed_params = first_params.replace(",m=1", ",m=0");
                    let full_chunk = format!("{};{}", fixed_params, accumulated_payload);

                    // Process the assembled chunk
                    let mut processor = image::ImageProcessor::new();
                    if let Some(cmd) = ansi::apc::parse_kitty_command(full_chunk.as_bytes()) {
                        let events = processor.process_kitty_command(&cmd, 0, 0);
                        let image_ready = events.iter().find_map(|e| {
                            if let image::ImageEvent::ImageReady { image } = e {
                                Some(image)
                            } else {
                                None
                            }
                        });
                        assert!(
                            image_ready.is_some(),
                            "Should decode image after frontend accumulation"
                        );
                        let decoded = image_ready.unwrap();
                        assert_eq!(decoded.width, 400);
                        assert_eq!(decoded.height, 400);
                    } else {
                        panic!("Failed to parse assembled chunk");
                    }
                }
            }
        }
    }

    /// Simulate tmux unwrapping: for each DCS passthrough block, strip
    /// the `ESC P tmux;` header and `ESC \` trailer, then undouble ESC bytes.
    fn simulate_tmux_unwrap(input: &str) -> String {
        let mut output = String::new();
        let bytes = input.as_bytes();
        let header = b"\x1bPtmux;";
        let mut i = 0;

        while i < bytes.len() {
            // Look for DCS passthrough header
            if i + header.len() <= bytes.len() && &bytes[i..i + header.len()] == header {
                let body_start = i + header.len();
                // Find DCS ST by scanning for single ESC followed by \
                // (doubled ESC-ESC is content, not terminator)
                let mut j = body_start;
                while j + 1 < bytes.len() {
                    if bytes[j] == 0x1B {
                        if j + 1 < bytes.len() && bytes[j + 1] == 0x1B {
                            // Doubled ESC: output single ESC, skip pair
                            output.push(0x1B as char);
                            j += 2;
                        } else if j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                            // DCS ST found: terminate this block
                            i = j + 2;
                            break;
                        } else {
                            // Bare ESC followed by something else
                            output.push(0x1B as char);
                            j += 1;
                        }
                    } else {
                        output.push(bytes[j] as char);
                        j += 1;
                    }
                }
                if j + 1 >= bytes.len() {
                    break;
                }
            } else {
                // Outside DCS passthrough: copy verbatim
                output.push(bytes[i] as char);
                i += 1;
            }
        }
        output
    }

    /// Extract APC bodies from a Kitty escape sequence string.
    /// Simulates what the WASM parser does: extract bytes between ESC_ and ESC\.
    fn extract_apc_bodies(sequence: &str) -> Vec<String> {
        let mut bodies = Vec::new();
        let bytes = sequence.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == 0x1B && bytes[i + 1] == b'_' {
                let start = i + 2;
                let mut j = start;
                while j + 1 < bytes.len() {
                    if bytes[j] == 0x1B && bytes[j + 1] == b'\\' {
                        bodies.push(String::from_utf8_lossy(&bytes[start..j]).to_string());
                        i = j + 2;
                        break;
                    }
                    j += 1;
                }
                if j + 1 >= bytes.len() {
                    break;
                }
            } else {
                i += 1;
            }
        }
        bodies
    }
}
