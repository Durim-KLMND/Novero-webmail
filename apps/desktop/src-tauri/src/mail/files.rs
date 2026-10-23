//! Files dragged onto the window, and pictures put into a message.
//!
//! The window takes file drops itself, so the UI gets their names and where
//! they landed, never their paths: it asks for a drop by number, which keeps
//! it from reading any file the person didn't drop or pick.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine;
use mail_core::{Error, Result};
use serde::Serialize;
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, Runtime, State, Window};
use tauri_plugin_dialog::DialogExt;

/// Sent to the UI while files are dragged over the window and when dropped.
pub const FILE_DROP: &str = "file-drop";

/// Pictures larger than this are not put into the text (they can still be attached).
const MAX_PICTURE: u64 = 10 * 1024 * 1024;

/// The files of the latest drop, until the next one.
#[derive(Default)]
pub struct Drops(Mutex<(u64, Vec<PathBuf>)>);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DropEvent {
    /// Files are being dragged over the window, at `x`, `y` (CSS pixels).
    Over {
        x: f64,
        y: f64,
    },
    Leave,
    /// Files were dropped. `id` names them for [`attach_dropped`] and
    /// [`dropped_picture`].
    Drop {
        id: u64,
        x: f64,
        y: f64,
        files: Vec<DroppedFile>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroppedFile {
    pub name: String,
    /// Whether it can go into the text as a picture.
    pub picture: bool,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "attachment".into())
}

/// The picture type of a file, by its name: one every mail client shows.
fn picture_type(path: &Path) -> Option<&'static str> {
    let kind = mime_guess::from_path(path).first()?;
    ["image/png", "image/jpeg", "image/gif", "image/webp"]
        .into_iter()
        .find(|t| *t == kind.essence_str())
}

/// Handles the window's drag-and-drop events.
pub fn window_event<R: Runtime>(window: &Window<R>, event: &DragDropEvent) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let ui = match event {
        DragDropEvent::Enter { position, .. } | DragDropEvent::Over { position } => {
            DropEvent::Over {
                x: position.x / scale,
                y: position.y / scale,
            }
        }
        DragDropEvent::Leave => DropEvent::Leave,
        DragDropEvent::Drop { paths, position } => {
            let files: Vec<PathBuf> = paths.iter().filter(|p| p.is_file()).cloned().collect();
            let drops = window.state::<Drops>();
            let mut latest = drops.0.lock().unwrap();
            latest.0 += 1;
            latest.1 = files.clone();
            DropEvent::Drop {
                id: latest.0,
                x: position.x / scale,
                y: position.y / scale,
                files: files
                    .iter()
                    .map(|p| DroppedFile {
                        name: name_of(p),
                        picture: picture_type(p).is_some(),
                    })
                    .collect(),
            }
        }
        _ => return,
    };
    // Only to the window the files were dropped on: each composer window
    // handles its own drops.
    let _ = window.emit_to(window.label(), FILE_DROP, ui);
}

fn dropped(drops: &Drops, id: u64) -> Result<Vec<PathBuf>> {
    let latest = drops.0.lock().unwrap();
    if latest.0 != id {
        return Err(Error::Invalid(
            "Those files were dropped earlier; drop them again.".into(),
        ));
    }
    Ok(latest.1.clone())
}

/// Attaches the dropped files to a draft. `only` picks some of them by
/// position (the rest went into the text as pictures).
#[tauri::command]
pub fn attach_dropped(
    state: State<'_, crate::state::AppState>,
    drops: State<'_, Drops>,
    id: i64,
    drop: u64,
    only: Option<Vec<usize>>,
) -> Result<mail_core::Draft> {
    let mut paths = dropped(&drops, drop)?;
    if let Some(only) = only {
        paths = only
            .into_iter()
            .filter_map(|i| paths.get(i).cloned())
            .collect();
    }
    crate::mail::compose::attach_paths(&state, id, paths)
}

/// A file as a `data:` URL, for putting a picture into the text.
fn picture_url(path: &Path) -> Result<String> {
    let name = name_of(path);
    let kind = picture_type(path).ok_or_else(|| {
        Error::Invalid(format!("{name} is not a picture that can go into the text"))
    })?;
    let size = std::fs::metadata(path)
        .map_err(|e| Error::Invalid(format!("{name} could not be read: {e}")))?
        .len();
    if size > MAX_PICTURE {
        return Err(Error::Invalid(format!(
            "{name} is too large to put into the text (over {} MB); attach it instead",
            MAX_PICTURE / (1024 * 1024)
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| Error::Invalid(format!("{name} could not be read: {e}")))?;
    Ok(format!(
        "data:{kind};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// The `index`th dropped file as a picture for the text.
#[tauri::command]
pub fn dropped_picture(drops: State<'_, Drops>, drop: u64, index: usize) -> Result<String> {
    let paths = dropped(&drops, drop)?;
    let path = paths
        .get(index)
        .ok_or_else(|| Error::Invalid("That file is not part of the drop.".into()))?;
    picture_url(path)
}

/// Asks for a picture with the system file picker. `None` when cancelled.
#[tauri::command]
pub async fn pick_picture(app: AppHandle) -> Result<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Insert picture")
        .add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    picture_url(&path).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_told_by_their_name() {
        assert_eq!(picture_type(Path::new("a/photo.JPG")), Some("image/jpeg"));
        assert_eq!(picture_type(Path::new("logo.png")), Some("image/png"));
        assert_eq!(picture_type(Path::new("drawing.svg")), None);
        assert_eq!(picture_type(Path::new("notes.txt")), None);
    }

    #[test]
    fn a_picture_becomes_a_data_url() {
        let dir = std::env::temp_dir().join(format!("novero-pic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("dot.gif");
        std::fs::write(&file, b"GIF89a").unwrap();
        assert_eq!(
            picture_url(&file).unwrap(),
            "data:image/gif;base64,R0lGODlh"
        );
        assert!(picture_url(&dir.join("x.txt")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
