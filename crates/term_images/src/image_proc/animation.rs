//! Animation frame and timing management.
//!
//! Handles Kitty animation frames and GIF animation playback.
//!
//! # Kitty Animation Protocol
//!
//! - `a=f` (Frame): Send animation frame data
//! - `a=a` (Animate): Control animation playback
//! - `a=c` (Compose): Compose frames together
//!
//! # Animation States
//!
//! - `s=1`: Stop animation
//! - `s=2`: Loading mode (waiting for more frames)
//! - `s=3`: Normal loop playback

use std::collections::HashMap;
use std::time::Instant;

use serde::Serialize;

/// Animation playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub enum AnimationState {
    /// Animation is stopped.
    #[default]
    Stopped,
    /// Loading mode - waiting for more frames.
    Loading,
    /// Normal playback (looping).
    Playing,
    /// Paused (e.g., off-screen).
    Paused,
}

/// Composition mode for frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub enum CompositionMode {
    /// Alpha blending (default).
    #[default]
    AlphaBlend,
    /// Replace pixels entirely.
    Replace,
}

/// A single animation frame.
#[derive(Debug, Clone, Serialize)]
pub struct AnimationFrame {
    /// Frame number (1-based).
    pub frame_number: u32,

    /// RGBA pixel data for this frame.
    #[serde(skip)]
    pub rgba_data: Vec<u8>,

    /// Base64-encoded RGBA data for IPC transfer.
    pub rgba_base64: String,

    /// Frame width in pixels.
    pub width: u32,

    /// Frame height in pixels.
    pub height: u32,

    /// Frame delay in milliseconds.
    pub delay_ms: u32,

    /// Background color (RGBA).
    pub background: Option<[u8; 4]>,

    /// Base frame number for composition.
    pub base_frame: Option<u32>,

    /// Composition mode.
    pub composition_mode: CompositionMode,

    /// Gap mode: if true, no gap between this and next frame.
    pub no_gap: bool,
}

impl AnimationFrame {
    /// Create a new frame with default settings.
    pub fn new(frame_number: u32, width: u32, height: u32, rgba_data: Vec<u8>) -> Self {
        let rgba_base64 = crate::image_proc::decoder::encode_base64(&rgba_data);
        Self {
            frame_number,
            rgba_data,
            rgba_base64,
            width,
            height,
            delay_ms: 40, // Default 25 FPS
            background: None,
            base_frame: None,
            composition_mode: CompositionMode::default(),
            no_gap: false,
        }
    }

    /// Set frame delay in milliseconds.
    pub fn with_delay(mut self, delay_ms: u32) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    /// Set background color.
    pub fn with_background(mut self, rgba: [u8; 4]) -> Self {
        self.background = Some(rgba);
        self
    }

    /// Set base frame for composition.
    pub fn with_base_frame(mut self, frame_number: u32) -> Self {
        self.base_frame = Some(frame_number);
        self
    }

    /// Set composition mode.
    pub fn with_composition_mode(mut self, mode: CompositionMode) -> Self {
        self.composition_mode = mode;
        self
    }

    /// Set gap mode.
    pub fn with_no_gap(mut self, no_gap: bool) -> Self {
        self.no_gap = no_gap;
        self
    }
}

/// Animation sequence containing multiple frames.
#[derive(Debug)]
pub struct Animation {
    /// Image ID this animation belongs to.
    pub image_id: u32,

    /// Animation frames (1-indexed).
    frames: HashMap<u32, AnimationFrame>,

    /// Current frame number.
    current_frame: u32,

    /// Total number of frames.
    frame_count: u32,

    /// Animation state.
    state: AnimationState,

    /// Loop count (0 = infinite).
    loop_count: u32,

    /// Current loop iteration.
    current_loop: u32,

    /// Last frame change time.
    #[allow(dead_code)]
    last_frame_time: Option<Instant>,
}

impl Animation {
    /// Create a new animation for an image.
    pub fn new(image_id: u32) -> Self {
        Self {
            image_id,
            frames: HashMap::new(),
            current_frame: 1,
            frame_count: 0,
            state: AnimationState::Loading,
            loop_count: 0,
            current_loop: 0,
            last_frame_time: None,
        }
    }

    /// Add a frame to the animation.
    pub fn add_frame(&mut self, frame: AnimationFrame) {
        let frame_number = frame.frame_number;
        self.frames.insert(frame_number, frame);
        if frame_number > self.frame_count {
            self.frame_count = frame_number;
        }
    }

    /// Get a frame by number (1-based).
    pub fn get_frame(&self, frame_number: u32) -> Option<&AnimationFrame> {
        self.frames.get(&frame_number)
    }

    /// Get the current frame.
    pub fn current_frame(&self) -> Option<&AnimationFrame> {
        self.frames.get(&self.current_frame)
    }

    /// Get current frame number.
    pub fn current_frame_number(&self) -> u32 {
        self.current_frame
    }

    /// Set the current frame number.
    pub fn set_current_frame(&mut self, frame_number: u32) {
        if frame_number > 0 && frame_number <= self.frame_count {
            self.current_frame = frame_number;
        }
    }

    /// Get frame count.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Get animation state.
    pub fn state(&self) -> AnimationState {
        self.state
    }

    /// Set animation state.
    pub fn set_state(&mut self, state: AnimationState) {
        self.state = state;
        if state == AnimationState::Playing {
            self.last_frame_time = Some(Instant::now());
        }
    }

    /// Set loop count (0 = infinite).
    pub fn set_loop_count(&mut self, count: u32) {
        self.loop_count = count;
    }

    /// Get loop count.
    pub fn loop_count(&self) -> u32 {
        self.loop_count
    }

    /// Check if animation is playing.
    pub fn is_playing(&self) -> bool {
        self.state == AnimationState::Playing
    }

    /// Check if animation has more frames to show.
    pub fn has_frames(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Advance to the next frame.
    ///
    /// Returns the new frame number, or None if animation should stop.
    pub fn advance_frame(&mut self) -> Option<u32> {
        if self.state != AnimationState::Playing || self.frame_count == 0 {
            return None;
        }

        let next_frame = self.current_frame + 1;

        if next_frame > self.frame_count {
            // Loop or stop
            if self.loop_count == 0 {
                // Infinite loop
                self.current_frame = 1;
                Some(1)
            } else {
                self.current_loop += 1;
                if self.current_loop >= self.loop_count {
                    // Animation complete
                    self.state = AnimationState::Stopped;
                    None
                } else {
                    self.current_frame = 1;
                    Some(1)
                }
            }
        } else {
            self.current_frame = next_frame;
            Some(next_frame)
        }
    }

    /// Get delay for current frame in milliseconds.
    pub fn current_frame_delay(&self) -> u32 {
        self.current_frame().map(|f| f.delay_ms).unwrap_or(40)
    }

    /// Compose a source frame onto a destination frame.
    pub fn compose_frames(&mut self, src_frame: u32, dst_frame: u32) -> Result<(), String> {
        // Get source frame data
        let src = self
            .frames
            .get(&src_frame)
            .ok_or_else(|| format!("Source frame {} not found", src_frame))?;

        let src_data = src.rgba_data.clone();
        let src_width = src.width;
        let src_height = src.height;
        let composition_mode = src.composition_mode;

        // Get destination frame
        let dst = self
            .frames
            .get_mut(&dst_frame)
            .ok_or_else(|| format!("Destination frame {} not found", dst_frame))?;

        // Validate dimensions match
        if src_width != dst.width || src_height != dst.height {
            return Err("Frame dimensions must match for composition".to_string());
        }

        // Perform composition
        match composition_mode {
            CompositionMode::Replace => {
                dst.rgba_data = src_data;
            }
            CompositionMode::AlphaBlend => {
                blend_rgba(&src_data, &mut dst.rgba_data);
            }
        }

        // Update base64
        dst.rgba_base64 = crate::image_proc::decoder::encode_base64(&dst.rgba_data);

        Ok(())
    }
}

/// Blend source RGBA data onto destination using alpha blending.
fn blend_rgba(src: &[u8], dst: &mut [u8]) {
    for (src_pixel, dst_pixel) in src.chunks(4).zip(dst.chunks_mut(4)) {
        let src_a = src_pixel[3] as f32 / 255.0;
        let dst_a = dst_pixel[3] as f32 / 255.0;

        // Porter-Duff over operation
        let out_a = src_a + dst_a * (1.0 - src_a);

        if out_a > 0.0 {
            for i in 0..3 {
                let src_c = src_pixel[i] as f32 / 255.0;
                let dst_c = dst_pixel[i] as f32 / 255.0;
                let out_c = (src_c * src_a + dst_c * dst_a * (1.0 - src_a)) / out_a;
                dst_pixel[i] = (out_c * 255.0) as u8;
            }
            dst_pixel[3] = (out_a * 255.0) as u8;
        }
    }
}

/// Animation manager that handles multiple animations.
#[derive(Debug, Default)]
pub struct AnimationManager {
    /// Animations by image ID.
    animations: HashMap<u32, Animation>,

    /// Pending frame events to send.
    pending_events: Vec<AnimationEvent>,
}

/// Animation events to send to frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AnimationEvent {
    /// Frame is ready to display.
    FrameReady {
        image_id: u32,
        frame_number: u32,
        delay_ms: u32,
        rgba_base64: String,
        width: u32,
        height: u32,
    },

    /// Animation state changed.
    StateChanged {
        image_id: u32,
        state: AnimationState,
    },

    /// Animation completed (all loops done).
    Completed { image_id: u32 },
}

impl AnimationManager {
    /// Create a new animation manager.
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
            pending_events: Vec::new(),
        }
    }

    /// Get or create an animation for an image.
    pub fn get_or_create(&mut self, image_id: u32) -> &mut Animation {
        self.animations
            .entry(image_id)
            .or_insert_with(|| Animation::new(image_id))
    }

    /// Get an animation by image ID.
    pub fn get(&self, image_id: u32) -> Option<&Animation> {
        self.animations.get(&image_id)
    }

    /// Get a mutable animation by image ID.
    pub fn get_mut(&mut self, image_id: u32) -> Option<&mut Animation> {
        self.animations.get_mut(&image_id)
    }

    /// Check if an animation exists.
    pub fn has_animation(&self, image_id: u32) -> bool {
        self.animations.contains_key(&image_id)
    }

    /// Remove an animation.
    pub fn remove(&mut self, image_id: u32) -> Option<Animation> {
        self.animations.remove(&image_id)
    }

    /// Clear all animations.
    pub fn clear(&mut self) {
        self.animations.clear();
        self.pending_events.clear();
    }

    /// Add a frame to an animation.
    pub fn add_frame(&mut self, image_id: u32, frame: AnimationFrame) -> Vec<AnimationEvent> {
        let animation = self.get_or_create(image_id);
        let frame_number = frame.frame_number;
        let delay_ms = frame.delay_ms;
        let rgba_base64 = frame.rgba_base64.clone();
        let width = frame.width;
        let height = frame.height;

        animation.add_frame(frame);

        vec![AnimationEvent::FrameReady {
            image_id,
            frame_number,
            delay_ms,
            rgba_base64,
            width,
            height,
        }]
    }

    /// Set animation state.
    pub fn set_state(&mut self, image_id: u32, state: AnimationState) -> Vec<AnimationEvent> {
        if let Some(animation) = self.animations.get_mut(&image_id) {
            animation.set_state(state);
            vec![AnimationEvent::StateChanged { image_id, state }]
        } else {
            vec![]
        }
    }

    /// Set the current frame for an animation.
    pub fn set_current_frame(&mut self, image_id: u32, frame_number: u32) -> Vec<AnimationEvent> {
        if let Some(animation) = self.animations.get_mut(&image_id) {
            animation.set_current_frame(frame_number);

            if let Some(frame) = animation.current_frame() {
                vec![AnimationEvent::FrameReady {
                    image_id,
                    frame_number,
                    delay_ms: frame.delay_ms,
                    rgba_base64: frame.rgba_base64.clone(),
                    width: frame.width,
                    height: frame.height,
                }]
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    }

    /// Set loop count for an animation.
    pub fn set_loop_count(&mut self, image_id: u32, count: u32) {
        if let Some(animation) = self.animations.get_mut(&image_id) {
            animation.set_loop_count(count);
        }
    }

    /// Advance all playing animations and return events.
    pub fn tick(&mut self) -> Vec<AnimationEvent> {
        let mut events = Vec::new();

        for (image_id, animation) in &mut self.animations {
            if animation.is_playing() {
                if let Some(frame_number) = animation.advance_frame() {
                    if let Some(frame) = animation.get_frame(frame_number) {
                        events.push(AnimationEvent::FrameReady {
                            image_id: *image_id,
                            frame_number,
                            delay_ms: frame.delay_ms,
                            rgba_base64: frame.rgba_base64.clone(),
                            width: frame.width,
                            height: frame.height,
                        });
                    }
                } else if animation.state() == AnimationState::Stopped {
                    events.push(AnimationEvent::Completed {
                        image_id: *image_id,
                    });
                }
            }
        }

        events
    }

    /// Compose frames in an animation.
    pub fn compose_frames(
        &mut self,
        image_id: u32,
        src_frame: u32,
        dst_frame: u32,
    ) -> Result<Vec<AnimationEvent>, String> {
        let animation = self
            .animations
            .get_mut(&image_id)
            .ok_or_else(|| format!("Animation {} not found", image_id))?;

        animation.compose_frames(src_frame, dst_frame)?;

        // Return updated frame event
        if let Some(frame) = animation.get_frame(dst_frame) {
            Ok(vec![AnimationEvent::FrameReady {
                image_id,
                frame_number: dst_frame,
                delay_ms: frame.delay_ms,
                rgba_base64: frame.rgba_base64.clone(),
                width: frame.width,
                height: frame.height,
            }])
        } else {
            Ok(vec![])
        }
    }

    /// Get animation count.
    pub fn len(&self) -> usize {
        self.animations.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.animations.is_empty()
    }
}

/// GIF frame information for decoded frames.
#[derive(Debug, Clone, Serialize)]
pub struct GifFrameInfo {
    /// Frame index (0-based).
    pub index: usize,

    /// Frame delay in milliseconds.
    pub delay_ms: u32,

    /// Frame width.
    pub width: u32,

    /// Frame height.
    pub height: u32,

    /// Frame left offset.
    pub left: u32,

    /// Frame top offset.
    pub top: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // AnimationFrame Tests
    // =========================================================================

    #[test]
    fn test_animation_frame_creation() {
        let rgba = vec![255, 0, 0, 255]; // 1x1 red pixel
        let frame = AnimationFrame::new(1, 1, 1, rgba.clone());

        assert_eq!(frame.frame_number, 1);
        assert_eq!(frame.width, 1);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.delay_ms, 40); // Default
        assert_eq!(frame.rgba_data, rgba);
        assert!(frame.background.is_none());
        assert!(frame.base_frame.is_none());
    }

    #[test]
    fn test_animation_frame_builder() {
        let rgba = vec![0; 4];
        let frame = AnimationFrame::new(2, 1, 1, rgba)
            .with_delay(100)
            .with_background([0, 0, 0, 255])
            .with_base_frame(1)
            .with_composition_mode(CompositionMode::Replace)
            .with_no_gap(true);

        assert_eq!(frame.frame_number, 2);
        assert_eq!(frame.delay_ms, 100);
        assert_eq!(frame.background, Some([0, 0, 0, 255]));
        assert_eq!(frame.base_frame, Some(1));
        assert_eq!(frame.composition_mode, CompositionMode::Replace);
        assert!(frame.no_gap);
    }

    // =========================================================================
    // Animation Tests
    // =========================================================================

    #[test]
    fn test_animation_creation() {
        let anim = Animation::new(1);

        assert_eq!(anim.image_id, 1);
        assert_eq!(anim.frame_count(), 0);
        assert_eq!(anim.current_frame_number(), 1);
        assert_eq!(anim.state(), AnimationState::Loading);
        assert!(!anim.has_frames());
    }

    #[test]
    fn test_animation_add_frames() {
        let mut anim = Animation::new(1);

        anim.add_frame(AnimationFrame::new(1, 10, 10, vec![0; 400]));
        anim.add_frame(AnimationFrame::new(2, 10, 10, vec![0; 400]));
        anim.add_frame(AnimationFrame::new(3, 10, 10, vec![0; 400]));

        assert_eq!(anim.frame_count(), 3);
        assert!(anim.has_frames());
        assert!(anim.get_frame(1).is_some());
        assert!(anim.get_frame(2).is_some());
        assert!(anim.get_frame(3).is_some());
        assert!(anim.get_frame(4).is_none());
    }

    #[test]
    fn test_animation_current_frame() {
        let mut anim = Animation::new(1);
        anim.add_frame(AnimationFrame::new(1, 10, 10, vec![0; 400]));
        anim.add_frame(AnimationFrame::new(2, 10, 10, vec![0; 400]));

        assert_eq!(anim.current_frame_number(), 1);
        assert!(anim.current_frame().is_some());

        anim.set_current_frame(2);
        assert_eq!(anim.current_frame_number(), 2);

        // Invalid frame number should be ignored
        anim.set_current_frame(99);
        assert_eq!(anim.current_frame_number(), 2);
    }

    #[test]
    fn test_animation_state_changes() {
        let mut anim = Animation::new(1);

        assert_eq!(anim.state(), AnimationState::Loading);
        assert!(!anim.is_playing());

        anim.set_state(AnimationState::Playing);
        assert_eq!(anim.state(), AnimationState::Playing);
        assert!(anim.is_playing());

        anim.set_state(AnimationState::Paused);
        assert_eq!(anim.state(), AnimationState::Paused);
        assert!(!anim.is_playing());

        anim.set_state(AnimationState::Stopped);
        assert_eq!(anim.state(), AnimationState::Stopped);
    }

    #[test]
    fn test_animation_advance_frame_infinite_loop() {
        let mut anim = Animation::new(1);
        anim.add_frame(AnimationFrame::new(1, 1, 1, vec![0; 4]));
        anim.add_frame(AnimationFrame::new(2, 1, 1, vec![0; 4]));
        anim.add_frame(AnimationFrame::new(3, 1, 1, vec![0; 4]));
        anim.set_state(AnimationState::Playing);
        anim.set_loop_count(0); // Infinite

        // Advance through frames
        assert_eq!(anim.current_frame_number(), 1);
        assert_eq!(anim.advance_frame(), Some(2));
        assert_eq!(anim.advance_frame(), Some(3));
        assert_eq!(anim.advance_frame(), Some(1)); // Loops back
        assert_eq!(anim.advance_frame(), Some(2));
    }

    #[test]
    fn test_animation_advance_frame_finite_loop() {
        let mut anim = Animation::new(1);
        anim.add_frame(AnimationFrame::new(1, 1, 1, vec![0; 4]));
        anim.add_frame(AnimationFrame::new(2, 1, 1, vec![0; 4]));
        anim.set_state(AnimationState::Playing);
        anim.set_loop_count(2); // 2 loops

        // First loop
        assert_eq!(anim.advance_frame(), Some(2));
        assert_eq!(anim.advance_frame(), Some(1)); // Loop 1 complete

        // Second loop
        assert_eq!(anim.advance_frame(), Some(2));
        assert_eq!(anim.advance_frame(), None); // Animation complete

        assert_eq!(anim.state(), AnimationState::Stopped);
    }

    #[test]
    fn test_animation_advance_frame_stopped() {
        let mut anim = Animation::new(1);
        anim.add_frame(AnimationFrame::new(1, 1, 1, vec![0; 4]));
        anim.set_state(AnimationState::Stopped);

        assert_eq!(anim.advance_frame(), None);
    }

    #[test]
    fn test_animation_frame_delay() {
        let mut anim = Animation::new(1);
        anim.add_frame(AnimationFrame::new(1, 1, 1, vec![0; 4]).with_delay(100));
        anim.add_frame(AnimationFrame::new(2, 1, 1, vec![0; 4]).with_delay(50));

        assert_eq!(anim.current_frame_delay(), 100);
        anim.set_current_frame(2);
        assert_eq!(anim.current_frame_delay(), 50);
    }

    // =========================================================================
    // Animation Composition Tests
    // =========================================================================

    #[test]
    fn test_animation_compose_frames_replace() {
        let mut anim = Animation::new(1);

        // Frame 1: Red pixel
        let frame1 = AnimationFrame::new(1, 1, 1, vec![255, 0, 0, 255]);
        anim.add_frame(frame1);

        // Frame 2: Blue pixel with replace mode
        let frame2 = AnimationFrame::new(2, 1, 1, vec![0, 0, 255, 255])
            .with_composition_mode(CompositionMode::Replace);
        anim.add_frame(frame2);

        // Compose frame 2 onto frame 1
        anim.compose_frames(2, 1).unwrap();

        let frame1 = anim.get_frame(1).unwrap();
        assert_eq!(frame1.rgba_data, vec![0, 0, 255, 255]); // Blue replaced red
    }

    #[test]
    fn test_animation_compose_frames_alpha_blend() {
        let mut anim = Animation::new(1);

        // Frame 1: Red pixel, fully opaque
        let frame1 = AnimationFrame::new(1, 1, 1, vec![255, 0, 0, 255]);
        anim.add_frame(frame1);

        // Frame 2: Blue pixel, 50% transparent, alpha blend mode
        let frame2 = AnimationFrame::new(2, 1, 1, vec![0, 0, 255, 128])
            .with_composition_mode(CompositionMode::AlphaBlend);
        anim.add_frame(frame2);

        // Compose frame 2 onto frame 1
        anim.compose_frames(2, 1).unwrap();

        let frame1 = anim.get_frame(1).unwrap();
        // Result should be blend of red and blue
        assert!(frame1.rgba_data[0] > 0); // Some red
        assert!(frame1.rgba_data[2] > 0); // Some blue
        assert_eq!(frame1.rgba_data[3], 255); // Full alpha
    }

    #[test]
    fn test_animation_compose_frames_dimension_mismatch() {
        let mut anim = Animation::new(1);

        anim.add_frame(AnimationFrame::new(1, 2, 2, vec![0; 16]));
        anim.add_frame(AnimationFrame::new(2, 1, 1, vec![0; 4]));

        let result = anim.compose_frames(2, 1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("dimensions"));
    }

    #[test]
    fn test_animation_compose_frames_not_found() {
        let mut anim = Animation::new(1);
        anim.add_frame(AnimationFrame::new(1, 1, 1, vec![0; 4]));

        let result = anim.compose_frames(99, 1);
        assert!(result.is_err());

        let result = anim.compose_frames(1, 99);
        assert!(result.is_err());
    }

    // =========================================================================
    // AnimationManager Tests
    // =========================================================================

    #[test]
    fn test_animation_manager_creation() {
        let manager = AnimationManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn test_animation_manager_get_or_create() {
        let mut manager = AnimationManager::new();

        let anim = manager.get_or_create(1);
        assert_eq!(anim.image_id, 1);

        assert!(manager.has_animation(1));
        assert!(!manager.has_animation(2));
    }

    #[test]
    fn test_animation_manager_add_frame() {
        let mut manager = AnimationManager::new();

        let frame = AnimationFrame::new(1, 10, 10, vec![0; 400]);
        let events = manager.add_frame(1, frame);

        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            AnimationEvent::FrameReady {
                image_id: 1,
                frame_number: 1,
                ..
            }
        ));
        assert!(manager.has_animation(1));
    }

    #[test]
    fn test_animation_manager_set_state() {
        let mut manager = AnimationManager::new();
        manager.get_or_create(1);

        let events = manager.set_state(1, AnimationState::Playing);

        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            AnimationEvent::StateChanged {
                image_id: 1,
                state: AnimationState::Playing
            }
        ));
    }

    #[test]
    fn test_animation_manager_set_current_frame() {
        let mut manager = AnimationManager::new();
        manager.add_frame(1, AnimationFrame::new(1, 1, 1, vec![0; 4]));
        manager.add_frame(1, AnimationFrame::new(2, 1, 1, vec![0; 4]));

        let events = manager.set_current_frame(1, 2);

        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            AnimationEvent::FrameReady {
                image_id: 1,
                frame_number: 2,
                ..
            }
        ));
    }

    #[test]
    fn test_animation_manager_tick() {
        let mut manager = AnimationManager::new();
        manager.add_frame(1, AnimationFrame::new(1, 1, 1, vec![0; 4]));
        manager.add_frame(1, AnimationFrame::new(2, 1, 1, vec![0; 4]));
        manager.set_state(1, AnimationState::Playing);

        let events = manager.tick();

        // Should advance to frame 2
        assert!(!events.is_empty());
        assert!(matches!(
            events[0],
            AnimationEvent::FrameReady {
                frame_number: 2,
                ..
            }
        ));
    }

    #[test]
    fn test_animation_manager_tick_stopped() {
        let mut manager = AnimationManager::new();
        manager.add_frame(1, AnimationFrame::new(1, 1, 1, vec![0; 4]));
        manager.set_state(1, AnimationState::Stopped);

        let events = manager.tick();
        assert!(events.is_empty());
    }

    #[test]
    fn test_animation_manager_compose() {
        let mut manager = AnimationManager::new();
        manager.add_frame(1, AnimationFrame::new(1, 1, 1, vec![255, 0, 0, 255]));
        manager.add_frame(
            1,
            AnimationFrame::new(2, 1, 1, vec![0, 0, 255, 255])
                .with_composition_mode(CompositionMode::Replace),
        );

        let events = manager.compose_frames(1, 2, 1).unwrap();

        assert!(!events.is_empty());
        assert!(matches!(
            events[0],
            AnimationEvent::FrameReady {
                frame_number: 1,
                ..
            }
        ));
    }

    #[test]
    fn test_animation_manager_remove() {
        let mut manager = AnimationManager::new();
        manager.get_or_create(1);
        assert!(manager.has_animation(1));

        let removed = manager.remove(1);
        assert!(removed.is_some());
        assert!(!manager.has_animation(1));
    }

    #[test]
    fn test_animation_manager_clear() {
        let mut manager = AnimationManager::new();
        manager.get_or_create(1);
        manager.get_or_create(2);

        manager.clear();
        assert!(manager.is_empty());
    }

    // =========================================================================
    // Alpha Blending Tests
    // =========================================================================

    #[test]
    fn test_blend_rgba_opaque_over_opaque() {
        let src = vec![255, 0, 0, 255]; // Red, opaque
        let mut dst = vec![0, 255, 0, 255]; // Green, opaque

        blend_rgba(&src, &mut dst);

        // Red should completely replace green
        assert_eq!(dst[0], 255); // Red
        assert_eq!(dst[1], 0); // No green
        assert_eq!(dst[2], 0); // No blue
        assert_eq!(dst[3], 255); // Full alpha
    }

    #[test]
    fn test_blend_rgba_transparent_over_opaque() {
        let src = vec![255, 0, 0, 0]; // Red, fully transparent
        let mut dst = vec![0, 255, 0, 255]; // Green, opaque

        blend_rgba(&src, &mut dst);

        // Green should remain unchanged
        assert_eq!(dst[0], 0);
        assert_eq!(dst[1], 255);
        assert_eq!(dst[2], 0);
        assert_eq!(dst[3], 255);
    }

    #[test]
    fn test_blend_rgba_semitransparent() {
        let src = vec![255, 0, 0, 128]; // Red, 50% alpha
        let mut dst = vec![0, 0, 255, 255]; // Blue, opaque

        blend_rgba(&src, &mut dst);

        // Should be blend of red and blue
        assert!(dst[0] > 100); // Some red
        assert!(dst[2] > 100); // Some blue
        assert_eq!(dst[3], 255); // Full alpha
    }
}
