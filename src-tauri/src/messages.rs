//! Opens a prepared reminder in the computer's own Messages, WhatsApp or email app (owner decision
//! 7 Nov 2026: reminders go from the therapist's own phone/computer; no SMS provider). The person still
//! presses Send there. Only `sms:`, `mailto:` and `https://wa.me/` links are allowed, so the UI can't
//! use this to open anything else.

fn allowed(url: &str) -> bool {
    let ok_scheme = url.starts_with("sms:") || url.starts_with("mailto:") || url.starts_with("https://wa.me/");
    ok_scheme && url.len() <= 2000 && !url.chars().any(|c| c.is_control() || c == ' ')
}

#[tauri::command]
pub fn open_message(url: String) -> Result<(), String> {
    if !allowed(&url) {
        return Err("This link can’t be opened.".into());
    }
    open::that_detached(&url).map_err(|_| "No app on this computer opens this kind of message.".into())
}

#[cfg(test)]
mod tests {
    use super::allowed;
    #[test]
    fn only_message_links() {
        assert!(allowed("sms:+351912345678?body=Ol%C3%A1"));
        assert!(allowed("https://wa.me/351912345678?text=Ol%C3%A1"));
        assert!(allowed("mailto:a@b.pt?subject=x&body=y"));
        assert!(!allowed("https://evil.example/"));
        assert!(!allowed("file:///etc/passwd"));
        assert!(!allowed("sms:1 && rm -rf /"));
        assert!(!allowed("javascript:alert(1)"));
    }
}
