//! In-app updates from GitHub Releases (owner, 25 Nov 2026). Desktop only.
//!
//! The UI calls `update_check` and `update_install`; tauri-plugin-updater does the rest:
//! - reads `latest.json` from the HTTPS endpoint in tauri.conf.json › plugins.updater (GitHub Releases),
//! - only offers a version newer than this one (no downgrades),
//! - downloads the update and checks its minisign signature against the public key compiled into the app
//!   (tauri.conf.json › plugins.updater.pubkey). A file that isn't signed with the owner's private key is
//!   refused before anything is installed, even if the download address or the release were tampered with.
//!
//! Network: this is the only code that talks to GitHub. It sends nothing but the HTTP request itself
//! (no clinic data, no patient data). How releases are made: claude/RELEASING.md.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize, Clone)]
pub struct UpdateInfo {
    /// The new version, e.g. "0.15.0".
    pub version: String,
    /// The version running now.
    pub current: String,
    /// The release notes from latest.json (the first paragraph of claude/releases/vX.Y.Z.md).
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Serialize, Clone)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

fn friendly(e: tauri_plugin_updater::Error) -> String {
    // The UI shows this under the button; keep the plugin's text for the details.
    format!("{e}")
}

/// `Ok(None)`: this is the newest version.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(friendly)?;
    let found = updater.check().await.map_err(friendly)?;
    Ok(found.map(|u| UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    }))
}

/// Downloads the update (progress goes out as the `update:progress` event), checks its signature,
/// installs it and restarts the app. A successful call doesn't return: on macOS the .app is swapped in
/// place and the app restarts; on Windows the installer takes over (passive mode) and starts it again.
/// The session ends either way (the token lives only in this process's memory).
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(friendly)?;
    let Some(update) = updater.check().await.map_err(friendly)? else {
        return Err("no_update".into());
    };
    let emitter = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = emitter.emit("update:progress", Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(friendly)?;
    app.restart()
}
