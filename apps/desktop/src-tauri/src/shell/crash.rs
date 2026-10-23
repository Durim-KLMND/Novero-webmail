//! Crash reports. A panic writes a short report to disk; nothing leaves the
//! computer unless the person chooses to send it the next time the app starts.
//!
//! Reports say where the code failed, not what mail was open: addresses,
//! quoted text (subjects, folder names, server replies) and the user's home
//! folder are taken out before anything is written.

use std::fs;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use mail_core::{Error, Result};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::settings::Settings;
use crate::state::AppState;

/// Sent reports become issues here, where the person can read them first.
const ISSUES_URL: &str = "https://github.com/Durim-KLMND/Novero-webmail/issues/new";
/// Browsers and GitHub refuse much longer links.
const MAX_SENT: usize = 6000;

/// Where reports wait, inside the app's data folder.
pub fn folder(data: &Path) -> PathBuf {
    data.join("crashes")
}

/// Writes a report for every panic, then runs the usual panic handling.
pub fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write(&dir, info);
        previous(info);
    }));
}

fn write(dir: &Path, info: &PanicHookInfo<'_>) -> std::io::Result<()> {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into());
    let at = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_default();
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    fs::create_dir_all(dir)?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    fs::write(
        dir.join(format!("crash-{secs}.txt")),
        report(&message, &at, &thread),
    )
}

fn report(message: &str, at: &str, thread: &str) -> String {
    scrub(&format!(
        "Novero Mail {} on {} {}\nThread: {thread}\nAt: {at}\n\n{message}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    ))
}

/// Takes out what could say something about the person or their mail.
pub fn scrub(text: &str) -> String {
    let text = home_paths(text);
    let text = quoted(&text);
    addresses(&text)
}

/// `"anything"` becomes `"…"`: Rust prints strings inside errors this way,
/// with `\"` for quotes within them.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    let mut escaped = false;
    for c in text.chars() {
        if inside && escaped {
            escaped = false;
        } else if inside && c == '\\' {
            escaped = true;
        } else if c == '"' {
            if inside {
                out.push('…');
            }
            inside = !inside;
            out.push(c);
        } else if !inside {
            out.push(c);
        }
    }
    if inside {
        out.push('…');
    }
    out
}

fn addresses(text: &str) -> String {
    let local = |c: char| c.is_alphanumeric() || "._%+-'".contains(c);
    let domain = |c: char| c.is_alphanumeric() || ".-".contains(c);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        let before = &rest[..at];
        let after = &rest[at + 1..];
        let start = before
            .char_indices()
            .rev()
            .take_while(|&(_, c)| local(c))
            .last()
            .map_or(before.len(), |(i, _)| i);
        let end = after.find(|c: char| !domain(c)).unwrap_or(after.len());
        let host = after[..end].trim_end_matches('.');
        if start < before.len() && host.contains('.') {
            out.push_str(&before[..start]);
            out.push_str("<address>");
            rest = &after[host.len()..];
        } else {
            out.push_str(&rest[..=at]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `C:\Users\name\…` and `/home/name/…` lose the name.
fn home_paths(text: &str) -> String {
    let mut out = text.to_owned();
    for prefix in [r"C:\Users\", r"c:\users\", "/home/", "/Users/"] {
        let mut from = 0;
        while let Some(i) = out[from..].find(prefix) {
            let name_at = from + i + prefix.len();
            let name_end = out[name_at..]
                .find(['\\', '/', ' ', '\n', ':'])
                .map_or(out.len(), |j| name_at + j);
            out.replace_range(name_at..name_end, "<user>");
            from = name_at + "<user>".len();
        }
    }
    out
}

/// The newest report, or none. Only one is offered; older ones are removed.
fn newest(dir: &Path) -> Option<(PathBuf, String)> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    let last = files.pop()?;
    for old in files {
        let _ = fs::remove_file(old);
    }
    let text = fs::read_to_string(&last).ok()?;
    Some((last, text))
}

fn clear(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
}

/// A report from a crash since the app last ran, when the person wants to be
/// asked about sending them. Otherwise any report is thrown away.
#[tauri::command]
pub fn crash_report(state: State<'_, AppState>) -> Option<String> {
    let dir = folder(&state.data_dir);
    if !Settings::load(&state.store.lock().unwrap()).crash_reports {
        clear(&dir);
        return None;
    }
    newest(&dir).map(|(_, text)| text)
}

/// Removes the waiting report without sending it.
#[tauri::command]
pub fn discard_crash_report(state: State<'_, AppState>) {
    clear(&folder(&state.data_dir));
}

/// Opens the report as a new GitHub issue in the browser, where the person
/// can read it before submitting.
#[tauri::command]
pub fn send_crash_report(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    let dir = folder(&state.data_dir);
    let Some((_, text)) = newest(&dir) else {
        return Ok(());
    };
    let mut body: String = text.chars().take(MAX_SENT).collect();
    body.push_str("\nWhat were you doing when it happened? (optional)\n");
    let url = tauri::Url::parse_with_params(
        ISSUES_URL,
        [
            (
                "title",
                format!("Crash report ({})", env!("CARGO_PKG_VERSION")),
            ),
            ("body", body),
        ],
    )
    .map_err(|e| Error::Invalid(e.to_string()))?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|e| Error::Invalid(format!("could not open the browser: {e}")))?;
    clear(&dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_leave_out_personal_details() {
        let text = report(
            r#"called `Result::unwrap()` on an `Err` value: Rejected("Mailbox \"Invoices 2026\" for ana.b@example.co.uk")"#,
            r"C:\Users\Durim\.cargo\registry\src\imap-1.0\src\lib.rs:12",
            "tokio-runtime-worker",
        );
        assert!(!text.contains("Invoices"), "{text}");
        assert!(!text.contains("ana.b"), "{text}");
        assert!(!text.contains("Durim"), "{text}");
        assert!(text.contains(r"C:\Users\<user>\.cargo"), "{text}");
        assert!(text.contains("Rejected(\"…"), "{text}");
        assert!(text.contains("tokio-runtime-worker"));
        assert!(text.starts_with(&format!("Novero Mail {}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn addresses_outside_quotes_are_removed() {
        assert_eq!(
            addresses("login for bo+mail@mail.example.com. failed; @ alone, a@b"),
            "login for <address>. failed; @ alone, a@b"
        );
        assert_eq!(
            home_paths("/home/runner/work/x.rs"),
            "/home/<user>/work/x.rs"
        );
    }

    #[test]
    fn a_panic_leaves_a_report() {
        let dir = std::env::temp_dir().join(format!("novero-panic-{}", std::process::id()));
        clear(&dir);
        install(dir.clone());
        let _ =
            std::panic::catch_unwind(|| panic!("sync of \"Receipts\" failed for ana@example.com"));
        let _ = std::panic::take_hook();
        let (_, text) = newest(&dir).expect("a report");
        assert!(
            text.contains("sync of \"…\" failed for <address>"),
            "{text}"
        );
        assert!(text.contains("crash.rs:"), "{text}");
        clear(&dir);
    }

    #[test]
    fn only_the_newest_report_is_kept() {
        let dir = std::env::temp_dir().join(format!("novero-crash-{}", std::process::id()));
        clear(&dir);
        assert!(newest(&dir).is_none());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("crash-100.txt"), "old").unwrap();
        fs::write(dir.join("crash-200.txt"), "new").unwrap();
        assert_eq!(newest(&dir).unwrap().1, "new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        clear(&dir);
    }
}
