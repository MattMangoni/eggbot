//! macOS notifications from bots. They need a bundle id, so they only work inside eggbot.app.

use std::sync::OnceLock;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, define_class, msg_send};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate};

use crate::tray::Action;

/// Where a click on a notification goes: the same channel as the menu bar egg.
static CLICKS: OnceLock<async_channel::Sender<Action>> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "EggbotNotifications"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn clicked(&self, _center: &UNUserNotificationCenter, response: &UNNotificationResponse, done: &DynBlock<dyn Fn()>) {
            let id = response.notification().request().identifier().to_string();
            if let (Some(tx), Some(bot)) = (CLICKS.get(), id.split(':').next().and_then(|n| n.parse().ok())) {
                let _ = tx.send_blocking(Action::Bot(bot));
            }
            done.call(());
        }
    }
);

fn bundled() -> bool {
    // without a bundle id, UNUserNotificationCenter raises an exception
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

/// Asks for permission (macOS shows it once) and routes clicks to `clicks`.
pub fn init(clicks: async_channel::Sender<Action>) {
    if !bundled() {
        return;
    }
    let _ = CLICKS.set(clicks);
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // the center holds its delegate weakly; this one lives as long as the app
    std::mem::forget(delegate);
    center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &RcBlock::new(|_: Bool, _: *mut NSError| {}));
}

/// Shows a notification; a click opens `bot`. Grouped per bot in Notification Center.
pub fn send(bot: usize, title: &str, body: &str) {
    if !bundled() {
        return;
    }
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setThreadIdentifier(&NSString::from_str(&format!("bot{bot}")));
    let id = format!("{bot}:{}", chrono::Utc::now().timestamp_millis());
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&id), &content, None);
    UNUserNotificationCenter::currentNotificationCenter().addNotificationRequest_withCompletionHandler(&request, None);
}
