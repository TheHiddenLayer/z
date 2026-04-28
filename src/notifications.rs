use notify_rust::Notification;

/// Fire-and-forget desktop notification. Errors are swallowed: a missing
/// notification daemon or a denied permission shouldn't disrupt the TUI.
pub fn fire(summary: &str, body: &str) {
    let _ = Notification::new()
        .summary(summary)
        .body(body)
        .sound_name("Glass")
        .show();
}
