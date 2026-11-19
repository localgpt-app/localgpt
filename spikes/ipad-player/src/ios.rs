//! The operating system's side: the audio session, the thermal state and the
//! battery, through objc2.

use objc2::MainThreadMarker;
use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryPlayback};
use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};
use objc2_ui_kit::{UIDevice, UIDeviceBatteryState};

/// Puts the app's audio session in the playback category and activates it.
///
/// cpal leaves the category to the app, and the default one, solo ambient,
/// is muted by the silent switch and stops when the screen locks. Playback
/// is what a music app sets: it plays through both, the second with
/// `UIBackgroundModes: audio` in the bundle's Info.plist.
pub fn start_playback_session() -> Result<(), String> {
    // SAFETY: the shared session is valid for the life of the process, the
    // category is a constant AVFAudio exports, and both setters are called
    // once, at start-up, before any audio stream exists.
    unsafe {
        let session = AVAudioSession::sharedInstance();
        let category = AVAudioSessionCategoryPlayback.ok_or("no playback category")?;
        session
            .setCategory_error(category)
            .map_err(|e| e.localizedDescription().to_string())?;
        session
            .setActive_error(true)
            .map_err(|e| e.localizedDescription().to_string())?;
    }
    Ok(())
}

/// The device's thermal state, as iOS reports it to apps.
pub fn thermal() -> &'static str {
    match NSProcessInfo::processInfo().thermalState() {
        NSProcessInfoThermalState::Nominal => "nominal",
        NSProcessInfoThermalState::Fair => "fair",
        NSProcessInfoThermalState::Serious => "serious",
        NSProcessInfoThermalState::Critical => "critical",
        _ => "unknown",
    }
}

/// The battery in percent and whether it charges, once iOS reports it
/// (never on the simulator). UIKit's device is the main thread's only.
pub fn battery() -> Option<(f32, &'static str)> {
    let mtm = MainThreadMarker::new()?;
    let device = UIDevice::currentDevice(mtm);
    device.setBatteryMonitoringEnabled(true);
    let level = device.batteryLevel();
    if level < 0.0 {
        return None;
    }
    let state = match device.batteryState() {
        UIDeviceBatteryState::Unplugged => "on battery",
        UIDeviceBatteryState::Charging => "charging",
        UIDeviceBatteryState::Full => "full",
        _ => "unknown",
    };
    Some((level * 100.0, state))
}
