//! Received attachments: their contents for the viewer, and saving them.

use std::path::PathBuf;

use mail_core::{Error, Result};
use tauri::AppHandle;
use tauri::ipc::Response;
use tauri_plugin_dialog::DialogExt;

use crate::commands::raw_message;

/// The `index`th attachment of a message, as `ParsedMessage::attachments`
/// lists it.
async fn find(
    app: &AppHandle,
    id: i64,
    index: usize,
) -> Result<(mail_mime::AttachmentInfo, Vec<u8>)> {
    let (_, raw) = raw_message(app, id).await?;
    tauri::async_runtime::spawn_blocking(move || mail_mime::attachment(&raw, index))
        .await
        .map_err(|e| Error::Protocol(e.to_string()))?
        .ok_or_else(|| Error::Protocol("the attachment is no longer in the message".into()))
}

/// The decoded contents of an attachment, as raw bytes (an `ArrayBuffer` in
/// the UI), for showing pictures, PDFs and text inside the app.
#[tauri::command]
pub async fn attachment_contents(app: AppHandle, id: i64, index: usize) -> Result<Response> {
    let (_, contents) = find(&app, id, index).await?;
    Ok(Response::new(contents))
}

/// Asks where to save an attachment and saves it there. False when the
/// person cancelled.
#[tauri::command]
pub async fn save_attachment(app: AppHandle, id: i64, index: usize) -> Result<bool> {
    let (info, contents) = find(&app, id, index).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Save attachment")
        .set_file_name(file_name(&info.name))
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(path) = rx.await.ok().flatten() else {
        return Ok(false);
    };
    let path: PathBuf = path
        .into_path()
        .map_err(|e| Error::Protocol(e.to_string()))?;
    std::fs::write(&path, contents)
        .map_err(|e| Error::Protocol(format!("{} could not be saved: {e}", info.name)))?;
    Ok(true)
}

/// A name the sender chose, made safe to suggest as a file name.
fn file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.');
    if cleaned.is_empty() {
        "attachment".into()
    } else {
        cleaned.into()
    }
}

#[cfg(test)]
mod tests {
    use super::file_name;

    #[test]
    fn suggested_names_have_no_paths() {
        assert_eq!(file_name("..\\..\\evil.exe"), "_.._evil.exe");
        assert_eq!(file_name("report:v2?.pdf"), "report_v2_.pdf");
        assert_eq!(file_name("  "), "attachment");
        assert_eq!(file_name("photo.jpg"), "photo.jpg");
    }
}
