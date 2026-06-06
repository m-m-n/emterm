//! BEL (0x07) audible-bell playback.
//!
//! `settings.bell_action = "sound"` mirrors the WebView build's
//! `handleBell` beep (`src/terminal-app/ui-handler.ts`): an 800 Hz sine
//! at gain 0.1 for 100 ms. Playback runs on a throwaway thread so the
//! UI loop never blocks on audio-device setup, and the default output
//! device is opened per beep instead of being held for the process
//! lifetime — bells are rare and holding the device hostage between
//! them is unfriendly to exclusive-mode audio setups.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Beep parameters matching the WebView build's `AudioContext`
/// oscillator (800 Hz, gain 0.1, 0.1 s).
const BEEP_FREQ_HZ: f32 = 800.0;
const BEEP_GAIN: f32 = 0.1;
const BEEP_DURATION: Duration = Duration::from_millis(100);

/// One beep at a time. A BEL burst (e.g. `yes $'\a'`) would otherwise
/// spawn a thread + audio stream per byte; overlapping 800 Hz sines are
/// indistinguishable from a single one anyway.
static BEEP_PLAYING: AtomicBool = AtomicBool::new(false);

/// Play the bell beep without blocking the caller. Errors (no audio
/// device, dead audio server, …) are warn-logged once per attempt — a
/// terminal must keep working on machines with no sound stack.
pub fn play_beep() {
    if BEEP_PLAYING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        if let Err(e) = beep_blocking() {
            log::warn!("bell: audible bell failed: {e}");
        }
        BEEP_PLAYING.store(false, Ordering::SeqCst);
    });
}

/// Open the default output device, play the beep, and wait for it to
/// finish so the stream isn't torn down mid-sine.
fn beep_blocking() -> Result<(), Box<dyn std::error::Error>> {
    use rodio::Source;
    let device = rodio::DeviceSinkBuilder::open_default_sink()?;
    let player = rodio::Player::connect_new(device.mixer());
    let source = rodio::source::SineWave::new(BEEP_FREQ_HZ)
        .take_duration(BEEP_DURATION)
        .amplify(BEEP_GAIN);
    player.append(source);
    player.sleep_until_end();
    Ok(())
}
