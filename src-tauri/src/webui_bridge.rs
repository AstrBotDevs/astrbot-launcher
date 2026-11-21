//! Init script injected into every frame of the launcher webview.
//!
//! It only activates inside the embedded AstrBot WebUI iframe (see
//! `webui_bridge.js`): it seeds the dashboard session and exposes the
//! `window.astrbotDesktop` bridge so the WebUI delegates restarts to the launcher.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

const WEBUI_BRIDGE_SCRIPT: &str = include_str!("webui_bridge.js");

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("webui-bridge")
        .js_init_script_on_all_frames(WEBUI_BRIDGE_SCRIPT)
        .build()
}
