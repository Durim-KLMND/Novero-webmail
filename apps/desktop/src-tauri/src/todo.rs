//! The To-do view: your own notes and task board, kept in the database.

use mail_core::{Error, Result};
use mail_store::{Board, Note, Task, TaskColumn};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub fn list_notes(state: State<'_, AppState>) -> Result<Vec<Note>> {
    state.store.lock().unwrap().notes()
}

/// Adds or changes a note, and returns it as stored.
#[tauri::command]
pub fn save_note(state: State<'_, AppState>, note: Note) -> Result<Note> {
    let note = Note {
        title: note.title.trim().to_owned(),
        body: note.body.replace("\r\n", "\n"),
        ..note
    };
    state.store.lock().unwrap().save_note(&note)
}

#[tauri::command]
pub fn delete_note(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().delete_note(id)
}

#[tauri::command]
pub fn task_board(state: State<'_, AppState>) -> Result<Board> {
    state.store.lock().unwrap().board()
}

/// Adds a column at the right end, or renames one.
#[tauri::command]
pub fn save_task_column(state: State<'_, AppState>, column: TaskColumn) -> Result<TaskColumn> {
    let column = TaskColumn {
        name: match column.name.trim() {
            "" => "Column".to_owned(),
            name => name.to_owned(),
        },
        ..column
    };
    state.store.lock().unwrap().save_column(&column)
}

#[tauri::command]
pub fn delete_task_column(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().delete_column(id)
}

#[tauri::command]
pub fn move_task_column(state: State<'_, AppState>, id: i64, index: usize) -> Result<()> {
    state.store.lock().unwrap().move_column(id, index)
}

/// Adds a card at the bottom of its column, or changes one.
#[tauri::command]
pub fn save_task(state: State<'_, AppState>, task: Task) -> Result<Task> {
    let title = task.title.trim();
    if title.is_empty() {
        return Err(Error::Invalid("give the task a name".into()));
    }
    let due = task.due.as_deref().map(str::trim).filter(|d| !d.is_empty());
    if let Some(day) = due
        && !is_day(day)
    {
        return Err(Error::Invalid(format!("{day} is not a date")));
    }
    let task = Task {
        title: title.to_owned(),
        notes: task.notes.trim_end().replace("\r\n", "\n"),
        due: due.map(str::to_owned),
        ..task
    };
    state.store.lock().unwrap().save_task(&task)
}

#[tauri::command]
pub fn move_task(state: State<'_, AppState>, id: i64, column: i64, index: usize) -> Result<()> {
    state.store.lock().unwrap().move_task(id, column, index)
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: i64) -> Result<()> {
    state.store.lock().unwrap().delete_task(id)
}

/// Deletes the finished cards in a column.
#[tauri::command]
pub fn clear_done_tasks(state: State<'_, AppState>, column: i64) -> Result<()> {
    state.store.lock().unwrap().clear_done_tasks(column)
}

/// YYYY-MM-DD, as a date input gives it.
fn is_day(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [y, m, d] = parts[..] else { return false };
    let number = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    number(y, 4)
        && number(m, 2)
        && number(d, 2)
        && (1..=12).contains(&m.parse::<u8>().unwrap_or(0))
        && (1..=31).contains(&d.parse::<u8>().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::is_day;

    #[test]
    fn days_look_like_date_inputs() {
        assert!(is_day("2026-10-22"));
        assert!(!is_day("2026-13-01"));
        assert!(!is_day("22/10/2026"));
        assert!(!is_day("2026-1-2"));
    }
}
