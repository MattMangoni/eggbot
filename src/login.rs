//! Start at login, through macOS's own login items (SMAppService). Only works inside eggbot.app.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

pub enum State {
    Off,
    On,
    /// Registered, but the user must allow it in System Settings → Login Items.
    NeedsApproval,
}

pub fn state() -> State {
    match unsafe { SMAppService::mainAppService().status() } {
        SMAppServiceStatus::Enabled => State::On,
        SMAppServiceStatus::RequiresApproval => State::NeedsApproval,
        _ => State::Off,
    }
}

pub fn set(on: bool) -> Result<(), String> {
    let service = unsafe { SMAppService::mainAppService() };
    let done = unsafe { if on { service.registerAndReturnError() } else { service.unregisterAndReturnError() } };
    done.map_err(|e| e.localizedDescription().to_string())
}

pub fn open_system_settings() {
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}

/// True when macOS opened eggbot as a login item. Only valid while the app finishes launching.
pub fn launched_at_login() -> bool {
    const fn code(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }
    let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else { return false };
    // raw sends: the typed accessors need the large objc2-core-services crate
    let id: u32 = unsafe { msg_send![&*event, eventID] };
    let prop: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![&*event, paramDescriptorForKeyword: code(b"prdt")] };
    id == code(b"oapp") && prop.is_some_and(|p| p.enumCodeValue() == code(b"lgit"))
}
