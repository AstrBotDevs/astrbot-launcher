use std::cell::RefCell;
use std::ptr::{self, NonNull};
use std::sync::Arc;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchTime, MainThreadBound};
use objc2::{rc::Retained, MainThreadMarker};
use objc2_app_kit::{NSWindow, NSWindowDidExitFullScreenNotification, NSWindowStyleMask};
use objc2_foundation::{NSNotification, NSNotificationCenter};

const FULLSCREEN_EXIT_TIMEOUT_NS: i64 = 5_000_000_000;

type PendingHide = Arc<MainThreadBound<Retained<NSWindow>>>;

thread_local! {
    static PENDING_HIDE: RefCell<Option<PendingHide>> = const { RefCell::new(None) };
}

pub fn setup() {
    let on_exit = RcBlock::new(|notification: NonNull<NSNotification>| {
        let Some(main_thread) = MainThreadMarker::new() else {
            return;
        };
        // The notification is live for the duration of this callback.
        let notification = unsafe { notification.as_ref() };
        let Some(window) = notification
            .object()
            .and_then(|object| object.downcast::<NSWindow>().ok())
        else {
            return;
        };
        let request = PENDING_HIDE.with_borrow(|pending| {
            pending
                .as_ref()
                .filter(|request| ptr::eq::<NSWindow>(&**request.get(main_thread), &*window))
                .map(Arc::clone)
        });
        if let Some(request) = request {
            // Let all observers finish before queuing the hide behind Tao's
            // asynchronous style and frame restoration on the main queue.
            DispatchQueue::main().exec_async(move || {
                DispatchQueue::main().exec_async(move || finish_hide(&request, false));
            });
        }
    });

    // The default center retains this app-wide observer for the process lifetime.
    // The block holds no window reference and also handles a recreated main window.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSWindowDidExitFullScreenNotification),
            None,
            None,
            &on_exit,
        );
    }
}

pub fn hide(window: &tauri::Window) {
    match defer_hide_until_fullscreen_exit(window) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => log::warn!("Could not defer hiding the main window: {error}"),
    }

    if let Err(error) = window.hide() {
        log::warn!("Failed to hide the main window: {error}");
    }
}

fn defer_hide_until_fullscreen_exit(window: &tauri::Window) -> tauri::Result<bool> {
    let Some(main_thread) = MainThreadMarker::new() else {
        return Err(std::io::Error::other("Window hiding requires the macOS main thread").into());
    };
    if PENDING_HIDE.with_borrow(Option::is_some) {
        return Ok(true);
    }

    // Tao can report windowed mode as soon as it requests an exit. The native
    // style also catches an exit already in progress when the user clicks close.
    // The Tauri window owns this native window on the main thread.
    let native_window = unsafe { Retained::retain(window.ns_window()?.cast::<NSWindow>()) }
        .ok_or_else(|| std::io::Error::other("Missing native window"))?;
    let fullscreen = window.is_fullscreen()?;
    if !fullscreen && !is_native_fullscreen(&native_window) {
        return Ok(false);
    }

    let request = Arc::new(MainThreadBound::new(native_window, main_thread));
    let timeout_request = Arc::downgrade(&request);
    // AppKit may refuse the transition without delivering an exit notification.
    // Only the current request may act, even after a reopen and another close.
    let deadline = DispatchTime::NOW.time(FULLSCREEN_EXIT_TIMEOUT_NS);
    DispatchQueue::main()
        .after(deadline, move || {
            if let Some(request) = timeout_request.upgrade() {
                finish_hide(&request, true);
            }
        })
        .map_err(|_| std::io::Error::other("Failed to schedule fullscreen exit timeout"))?;

    if fullscreen {
        let window = window.clone();
        let exit_request = Arc::downgrade(&request);
        // Window-state restoration can already have queued a fullscreen entry.
        // Let Tao start it before asking Tao to exit.
        DispatchQueue::main().exec_async(move || {
            if !exit_request
                .upgrade()
                .is_some_and(|request| is_current_hide(&request))
            {
                return;
            }
            if let Err(error) = window.set_fullscreen(false) {
                log::warn!("Failed to request fullscreen exit: {error}");
            }
        });
    }
    PENDING_HIDE.replace(Some(request));
    Ok(true)
}

pub fn cancel_pending_hide() {
    PENDING_HIDE.take();
}

fn finish_hide(request: &PendingHide, timed_out: bool) {
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    if !is_current_hide(request) {
        return;
    }

    let window = request.get(main_thread);
    if !is_native_fullscreen(window) {
        cancel_pending_hide();
        window.orderOut(None);
    } else if timed_out {
        cancel_pending_hide();
        log::warn!("The main window did not exit fullscreen; attempting to minimize it");
        window.miniaturize(None);
    }
}

fn is_current_hide(request: &PendingHide) -> bool {
    PENDING_HIDE.with_borrow(|pending| {
        pending
            .as_ref()
            .is_some_and(|pending| Arc::ptr_eq(pending, request))
    })
}

fn is_native_fullscreen(window: &NSWindow) -> bool {
    window.styleMask().contains(NSWindowStyleMask::FullScreen)
}
