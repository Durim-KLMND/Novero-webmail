//! Your own notes, and a task board of columns and cards. Kept on this
//! computer only; nothing here is synced to a mail account.

use mail_core::{Error, Result};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::storage;
use crate::store::Store;

/// A note. `id` is 0 for one not saved yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: i64,
    pub title: String,
    /// Plain text.
    pub body: String,
    pub pinned: bool,
    /// When the title or text last changed, in seconds since 1970.
    #[serde(default)]
    pub updated: i64,
}

/// A column on the board. `id` is 0 for one not saved yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskColumn {
    pub id: i64,
    pub name: String,
}

/// A card on the board. `id` is 0 for one not saved yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: i64,
    pub column: i64,
    pub title: String,
    pub notes: String,
    /// The day it is due, as YYYY-MM-DD.
    pub due: Option<String>,
    pub done: bool,
}

/// The whole board: columns left to right, and every card, top to bottom
/// within its column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub columns: Vec<TaskColumn>,
    pub tasks: Vec<Task>,
}

impl Store {
    /// Every note: pinned ones first, then the most recently changed.
    pub fn notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, title, body, pinned, updated_at FROM notes \
                 ORDER BY pinned DESC, updated_at DESC, id DESC",
            )
            .map_err(storage)?;
        let rows = stmt.query_map([], note_row).map_err(storage)?;
        rows.collect::<rusqlite::Result<_>>().map_err(storage)
    }

    /// Adds the note (id 0) or changes it, and returns it as stored. Only a
    /// change to its title or text moves it up the list.
    pub fn save_note(&self, note: &Note) -> Result<Note> {
        let id = if note.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO notes (title, body, pinned) VALUES (?1, ?2, ?3)",
                    params![note.title, note.body, note.pinned],
                )
                .map_err(storage)?;
            self.conn.last_insert_rowid()
        } else {
            let changed = self
                .conn
                .execute(
                    "UPDATE notes SET title = ?2, body = ?3, pinned = ?4, \
                     updated_at = CASE WHEN title <> ?2 OR body <> ?3 THEN unixepoch() \
                         ELSE updated_at END \
                     WHERE id = ?1",
                    params![note.id, note.title, note.body, note.pinned],
                )
                .map_err(storage)?;
            if changed == 0 {
                return Err(Error::Invalid("that note no longer exists".into()));
            }
            note.id
        };
        self.conn
            .query_row(
                "SELECT id, title, body, pinned, updated_at FROM notes WHERE id = ?1",
                [id],
                note_row,
            )
            .map_err(storage)
    }

    pub fn delete_note(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM notes WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// The board's columns and cards, in order.
    pub fn board(&self) -> Result<Board> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM task_columns ORDER BY position, id")
            .map_err(storage)?;
        let columns = stmt
            .query_map([], |r| {
                Ok(TaskColumn {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.id, t.column_id, t.title, t.notes, t.due, t.done FROM tasks t \
                 JOIN task_columns c ON c.id = t.column_id \
                 ORDER BY c.position, c.id, t.position, t.id",
            )
            .map_err(storage)?;
        let tasks = stmt
            .query_map([], task_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        Ok(Board { columns, tasks })
    }

    /// Adds the column at the right end (id 0) or renames it.
    pub fn save_column(&self, column: &TaskColumn) -> Result<TaskColumn> {
        let mut saved = column.clone();
        if column.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO task_columns (name, position) \
                     VALUES (?1, (SELECT coalesce(max(position) + 1, 0) FROM task_columns))",
                    [&column.name],
                )
                .map_err(storage)?;
            saved.id = self.conn.last_insert_rowid();
        } else {
            let changed = self
                .conn
                .execute(
                    "UPDATE task_columns SET name = ?2 WHERE id = ?1",
                    params![column.id, column.name],
                )
                .map_err(storage)?;
            if changed == 0 {
                return Err(Error::Invalid("that column no longer exists".into()));
            }
        }
        Ok(saved)
    }

    /// Deletes a column and the cards in it. The board keeps at least one.
    pub fn delete_column(&self, id: i64) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM task_columns", [], |r| r.get(0))
            .map_err(storage)?;
        if count <= 1 {
            return Err(Error::Invalid("the board needs at least one column".into()));
        }
        self.conn
            .execute("DELETE FROM task_columns WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Moves a column to `index` (0 is the left end).
    pub fn move_column(&self, id: i64, index: usize) -> Result<()> {
        let mut ids: Vec<i64> = self
            .conn
            .prepare("SELECT id FROM task_columns ORDER BY position, id")
            .map_err(storage)?
            .query_map([], |r| r.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        let Some(from) = ids.iter().position(|&c| c == id) else {
            return Err(Error::Invalid("that column no longer exists".into()));
        };
        ids.remove(from);
        ids.insert(index.min(ids.len()), id);
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        for (position, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE task_columns SET position = ?1 WHERE id = ?2",
                params![position as i64, id],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }

    /// Adds the card at the bottom of its column (id 0), or changes its
    /// title, notes, due day and whether it is done. Moving it to another
    /// column is `move_task`.
    pub fn save_task(&self, task: &Task) -> Result<Task> {
        let id = if task.id == 0 {
            self.column_exists(task.column)?;
            self.conn
                .execute(
                    "INSERT INTO tasks (column_id, title, notes, due, done, position) \
                     VALUES (?1, ?2, ?3, ?4, ?5, \
                         (SELECT coalesce(max(position) + 1, 0) FROM tasks WHERE column_id = ?1))",
                    params![task.column, task.title, task.notes, task.due, task.done],
                )
                .map_err(storage)?;
            self.conn.last_insert_rowid()
        } else {
            let changed = self
                .conn
                .execute(
                    "UPDATE tasks SET title = ?2, notes = ?3, due = ?4, done = ?5 WHERE id = ?1",
                    params![task.id, task.title, task.notes, task.due, task.done],
                )
                .map_err(storage)?;
            if changed == 0 {
                return Err(Error::Invalid("that task no longer exists".into()));
            }
            task.id
        };
        self.conn
            .query_row(
                "SELECT id, column_id, title, notes, due, done FROM tasks WHERE id = ?1",
                [id],
                task_row,
            )
            .map_err(storage)
    }

    /// Puts the card in `column` at `index` (0 is the top).
    pub fn move_task(&self, id: i64, column: i64, index: usize) -> Result<()> {
        self.column_exists(column)?;
        let from: Option<i64> = self
            .conn
            .query_row("SELECT column_id FROM tasks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)?;
        if from.is_none() {
            return Err(Error::Invalid("that task no longer exists".into()));
        }
        let mut ids: Vec<i64> = self
            .conn
            .prepare("SELECT id FROM tasks WHERE column_id = ?1 AND id <> ?2 ORDER BY position, id")
            .map_err(storage)?
            .query_map(params![column, id], |r| r.get(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        ids.insert(index.min(ids.len()), id);
        let tx = self.conn.unchecked_transaction().map_err(storage)?;
        for (position, task) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE tasks SET column_id = ?1, position = ?2 WHERE id = ?3",
                params![column, position as i64, task],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)
    }

    pub fn delete_task(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM tasks WHERE id = ?1", [id])
            .map_err(storage)?;
        Ok(())
    }

    /// Deletes the cards marked done in a column.
    pub fn clear_done_tasks(&self, column: i64) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM tasks WHERE column_id = ?1 AND done = 1",
                [column],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn column_exists(&self, column: i64) -> Result<()> {
        let found: Option<i64> = self
            .conn
            .query_row("SELECT id FROM task_columns WHERE id = ?1", [column], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)?;
        match found {
            Some(_) => Ok(()),
            None => Err(Error::Invalid("that column no longer exists".into())),
        }
    }
}

fn note_row(r: &Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        pinned: r.get(3)?,
        updated: r.get(4)?,
    })
}

fn task_row(r: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        column: r.get(1)?,
        title: r.get(2)?,
        notes: r.get(3)?,
        due: r.get(4)?,
        done: r.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory(dir.path()).unwrap();
        (dir, store)
    }

    fn note(title: &str) -> Note {
        Note {
            id: 0,
            title: title.into(),
            body: String::new(),
            pinned: false,
            updated: 0,
        }
    }

    fn task(column: i64, title: &str) -> Task {
        Task {
            id: 0,
            column,
            title: title.into(),
            notes: String::new(),
            due: None,
            done: false,
        }
    }

    fn titles(store: &Store, column: i64) -> Vec<String> {
        store
            .board()
            .unwrap()
            .tasks
            .into_iter()
            .filter(|t| t.column == column)
            .map(|t| t.title)
            .collect()
    }

    #[test]
    fn notes_list_pinned_first() {
        let (_dir, store) = store();
        let a = store.save_note(&note("Shopping")).unwrap();
        let mut b = store.save_note(&note("Ideas")).unwrap();
        assert!(a.updated > 0);
        b.pinned = true;
        b.body = "A mail client".into();
        let b = store.save_note(&b).unwrap();
        let order: Vec<_> = store.notes().unwrap().into_iter().map(|n| n.id).collect();
        assert_eq!(order, [b.id, a.id]);

        store.delete_note(b.id).unwrap();
        assert_eq!(store.notes().unwrap(), [a.clone()]);
        assert!(store.save_note(&b).is_err());
    }

    #[test]
    fn the_board_starts_with_three_columns() {
        let (_dir, store) = store();
        let names: Vec<_> = store
            .board()
            .unwrap()
            .columns
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["To do", "In progress", "Done"]);
    }

    #[test]
    fn cards_move_between_columns() {
        let (_dir, store) = store();
        let board = store.board().unwrap();
        let (todo, doing) = (board.columns[0].id, board.columns[1].id);
        let a = store.save_task(&task(todo, "Write tests")).unwrap();
        let b = store.save_task(&task(todo, "Fix bug")).unwrap();
        let c = store.save_task(&task(todo, "Release")).unwrap();
        assert_eq!(titles(&store, todo), ["Write tests", "Fix bug", "Release"]);

        store.move_task(c.id, todo, 0).unwrap();
        assert_eq!(titles(&store, todo), ["Release", "Write tests", "Fix bug"]);
        store.move_task(a.id, doing, 5).unwrap();
        assert_eq!(titles(&store, todo), ["Release", "Fix bug"]);
        assert_eq!(titles(&store, doing), ["Write tests"]);

        let mut done = b.clone();
        done.done = true;
        done.due = Some("2026-10-30".into());
        assert_eq!(store.save_task(&done).unwrap(), done);
        store.clear_done_tasks(todo).unwrap();
        assert_eq!(titles(&store, todo), ["Release"]);

        assert!(store.save_task(&task(999, "Nowhere")).is_err());
        assert!(store.move_task(c.id, 999, 0).is_err());
    }

    #[test]
    fn columns_reorder_and_take_their_cards_with_them() {
        let (_dir, store) = store();
        let board = store.board().unwrap();
        let ids: Vec<_> = board.columns.iter().map(|c| c.id).collect();
        let later = store
            .save_column(&TaskColumn {
                id: 0,
                name: "Later".into(),
            })
            .unwrap();
        store.move_column(later.id, 0).unwrap();
        let order: Vec<_> = store
            .board()
            .unwrap()
            .columns
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(order, [later.id, ids[0], ids[1], ids[2]]);

        store.save_task(&task(later.id, "Someday")).unwrap();
        store.delete_column(later.id).unwrap();
        assert!(store.board().unwrap().tasks.is_empty());

        store.delete_column(ids[0]).unwrap();
        store.delete_column(ids[1]).unwrap();
        assert!(store.delete_column(ids[2]).is_err());
    }
}
