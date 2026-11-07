//! `UNUserNotificationCenter`. It needs a `.app` bundle with an identifier: a
//! bare binary, as `tauri dev` runs, would stop on the first call. So every
//! entry checks [`available`] first.

use std::sync::{Mutex, OnceLock};

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_foundation::{NSArray, NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::{oneshot, OnceCell};

use super::{session_of, CLICK_EVENT};

/// True when this app is a `.app` bundle with an identifier.
fn available() -> bool {
    static BUNDLED: OnceLock<bool> = OnceLock::new();
    *BUNDLED.get_or_init(|| {
        let bundle = NSBundle::mainBundle();
        bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
    })
}

struct Ivars {
    app: AppHandle,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Delegate has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "SkiffNotifyDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Shows the banner while Skiff is the active app too: the page sends
        /// only for a pane that does not have the keys.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            done.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        /// A click: the page brings the window forward and opens the pane.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            done: &DynBlock<dyn Fn()>,
        ) {
            let request = response.notification().request().identifier().to_string();
            if let Some(session) = session_of(&request) {
                let _ = self.ivars().app.emit(CLICK_EVENT, session);
            }
            done.call(());
        }
    }
);

/// Sets the click handler. The center keeps only a weak reference, so the
/// delegate lives as long as the app.
pub(super) fn install(app: &AppHandle) {
    if !available() {
        eprintln!("notify: not a bundled app, notifications go through the plugin");
        return;
    }
    eprintln!("notify: notifications go through UNUserNotificationCenter");
    let delegate = Delegate::alloc().set_ivars(Ivars { app: app.clone() });
    // SAFETY: NSObject's init on a newly allocated object.
    let delegate: Retained<Delegate> = unsafe { msg_send![super(delegate), init] };
    UNUserNotificationCenter::currentNotificationCenter().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    std::mem::forget(delegate);
}

/// Asks for permission once, and waits for the answer. Later calls wait for
/// the same answer. A refusal shows as an error when a request is added.
async fn authorize() {
    static ASKED: OnceCell<()> = OnceCell::const_new();
    ASKED
        .get_or_init(|| async {
            if !ask().await.unwrap_or(false) {
                eprintln!("notify: permission not given");
            }
        })
        .await;
}

fn ask() -> oneshot::Receiver<bool> {
    let (tx, rx) = oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let answer = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(granted.as_bool());
        }
    });
    UNUserNotificationCenter::currentNotificationCenter().requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &answer,
    );
    rx
}

pub(super) async fn show(request: &str, thread: &str, title: &str, subtitle: &str, body: &str) -> Result<(), String> {
    if !available() {
        return Err("Not a bundled app.".into());
    }
    authorize().await;
    add(request, thread, title, subtitle, body).await.unwrap_or_else(|_| Err("No answer from the notification center.".into()))
}

fn add(request: &str, thread: &str, title: &str, subtitle: &str, body: &str) -> oneshot::Receiver<Result<(), String>> {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    if !subtitle.is_empty() {
        content.setSubtitle(&NSString::from_str(subtitle));
    }
    content.setBody(&NSString::from_str(body));
    content.setThreadIdentifier(&NSString::from_str(thread));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(request), &content, None);

    let (tx, rx) = oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let added = RcBlock::new(move |error: *mut NSError| {
        // SAFETY: the center passes null or a valid NSError.
        let result = match unsafe { error.as_ref() } {
            None => Ok(()),
            Some(e) => Err(e.localizedDescription().to_string()),
        };
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(result);
        }
    });
    UNUserNotificationCenter::currentNotificationCenter().addNotificationRequest_withCompletionHandler(&request, Some(&added));
    rx
}

pub(super) fn remove(request: &str) {
    if !available() {
        return;
    }
    let id = NSString::from_str(request);
    let ids = NSArray::from_slice(&[&*id]);
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.removePendingNotificationRequestsWithIdentifiers(&ids);
    center.removeDeliveredNotificationsWithIdentifiers(&ids);
}
