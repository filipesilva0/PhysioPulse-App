//! The staff iPhone app (PhysioPulse plan, phase 5): the session token behind Face ID.
//!
//! - After password, two-step code and the owner's approval, the server gives a **phone session**: 30 days, no
//!   idle timeout (`server/src/auth.rs`). The token is saved in the iPhone's keychain with an access control that
//!   needs Face ID (or the phone's passcode) to read it, only on this phone (never in iCloud or backups), and only
//!   while the phone has a passcode.
//! - Every time the app comes back to the screen, the UI calls `phone_lock` when it leaves and `phone_unlock` when it
//!   returns: iOS shows the Face ID prompt, and only then is the token read back into memory. The UI never sees it.
//! - `phone.json` in the app's folder holds who is signed in (name and email, for the lock screen). Not secret.
//! - Signing out, the owner signing the phone out in Team, or the 30 days running out: the keychain item is deleted.
//!
//! On other systems these commands exist but report that there is no phone (the desktop app never calls them).

use crate::server::{ApiError, Conn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

/// Keychain service and account for the session token.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
const SERVICE: &str = "app.physiopulse.session";
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
const ACCOUNT: &str = "staff";

/// Who is signed in on this phone (no token).
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct PhoneUser {
    pub name: String,
    pub email: String,
    pub server: String,
}

fn user_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("phone.json"))
}
fn load_user(app: &AppHandle) -> Option<PhoneUser> {
    user_path(app).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok())
}

fn local(code: &str, message: &str) -> ApiError {
    ApiError::local(code, message)
}

// ───────── the keychain (iOS only) ─────────

#[cfg(target_os = "ios")]
mod keychain {
    use super::{ACCOUNT, SERVICE};
    use core_foundation::{base::TCFType, string::{CFString, CFStringRef}};
    use security_framework::access_control::{ProtectionMode, SecAccessControl};
    use security_framework::passwords::{delete_generic_password, generic_password, set_generic_password_options, AccessControlOptions, PasswordOptions};

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        static kSecUseOperationPrompt: CFStringRef;
    }

    /// errSecUserCanceled, errSecAuthFailed, errSecItemNotFound, errSecInteractionNotAllowed.
    pub const CANCELLED: i32 = -128;
    pub const AUTH_FAILED: i32 = -25293;
    pub const NOT_FOUND: i32 = -25300;
    pub const NOT_ALLOWED: i32 = -25308;

    /// Saves the token so that reading it needs Face ID or the passcode, on this phone only, while it has a passcode.
    pub fn save(token: &str) -> Result<(), i32> {
        let _ = delete_generic_password(SERVICE, ACCOUNT);
        let ac = SecAccessControl::create_with_protection(Some(ProtectionMode::AccessibleWhenPasscodeSetThisDeviceOnly), AccessControlOptions::USER_PRESENCE.bits())
            .map_err(|e| e.code())?;
        let mut o = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
        o.set_access_control(ac);
        set_generic_password_options(token.as_bytes(), o).map_err(|e| e.code())
    }

    /// Reads it back: iOS shows Face ID (or asks for the passcode) with `prompt` as the reason.
    pub fn read(prompt: &str) -> Result<String, i32> {
        let mut o = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
        #[allow(deprecated)]
        o.query.push((unsafe { CFString::wrap_under_get_rule(kSecUseOperationPrompt) }, CFString::new(prompt).into_CFType()));
        let bytes = generic_password(o).map_err(|e| e.code())?;
        String::from_utf8(bytes).map_err(|_| NOT_FOUND)
    }

    pub fn delete() {
        let _ = delete_generic_password(SERVICE, ACCOUNT);
    }
}

/// After a sign-in on the phone: keep the token behind Face ID and remember who it is.
#[cfg(target_os = "ios")]
pub fn keep(app: &AppHandle, token: &str, user: PhoneUser) {
    if keychain::save(token).is_ok() {
        if let Some(p) = user_path(app) {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, serde_json::to_string(&user).unwrap_or_default());
        }
    }
}
#[cfg(not(target_os = "ios"))]
pub fn keep(_app: &AppHandle, _token: &str, _user: PhoneUser) {}

/// The session is over (signed out, signed out by the owner, 30 days, phone removed): forget it here too.
pub fn forget(app: &AppHandle) {
    #[cfg(target_os = "ios")]
    keychain::delete();
    if let Some(p) = user_path(app) {
        let _ = std::fs::remove_file(p);
    }
}

// ───────── commands for the phone UI ─────────

/// Who is signed in on this phone, if anyone (for the lock screen). `None`: show the sign-in screen.
#[tauri::command]
pub fn phone_user(app: AppHandle, conn: State<'_, Conn>) -> Option<PhoneUser> {
    let base = conn.base.lock().unwrap().clone()?;
    load_user(&app).filter(|u| u.server == base)
}

/// Face ID (or the passcode), then the token goes back into memory. Errors: `unlock_cancelled` (try again),
/// `signed_out` (nothing saved any more: sign in), `no_phone` (not an iPhone).
/// Async and off the main thread: reading the keychain waits for Face ID, and on the main thread that would freeze
/// the app (iOS couldn't even show the Face ID sheet).
#[tauri::command]
pub async fn phone_unlock(app: AppHandle, conn: State<'_, Conn>, prompt: String) -> Result<(), ApiError> {
    #[cfg(target_os = "ios")]
    {
        let prompt: String = prompt.chars().take(120).collect();
        let read = tauri::async_runtime::spawn_blocking(move || keychain::read(&prompt)).await.unwrap_or(Err(keychain::NOT_ALLOWED));
        return match read {
            Ok(t) => {
                *conn.token.lock().unwrap() = Some(t);
                Ok(())
            }
            Err(keychain::CANCELLED) | Err(keychain::AUTH_FAILED) | Err(keychain::NOT_ALLOWED) => Err(local("unlock_cancelled", "Unlock PhysioPulse with Face ID to continue.")),
            Err(_) => {
                forget(&app);
                Err(local("signed_out", "Sign in again."))
            }
        };
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (app, conn, prompt);
        Err(local("no_phone", "This only works on the iPhone."))
    }
}

/// The app left the screen: drop the token from memory, so coming back needs Face ID again.
#[tauri::command]
pub fn phone_lock(conn: State<'_, Conn>) {
    *conn.token.lock().unwrap() = None;
}

/// "Sign Out" on the phone: ends the session on the server (best effort) and forgets it here.
#[tauri::command]
pub async fn phone_sign_out(app: AppHandle, conn: State<'_, Conn>) -> Result<(), ApiError> {
    crate::server::logout_now(&conn).await;
    forget(&app);
    Ok(())
}
