-- Your own notes and a task board, kept on this computer only.

-- A note: a title and plain text. Pinned ones are listed first.
CREATE TABLE notes (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    pinned INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- The board's columns, left to right.
CREATE TABLE task_columns (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0
);

-- A card on the board, top to bottom in its column. `due` is a day
-- (YYYY-MM-DD) or NULL.
CREATE TABLE tasks (
    id INTEGER PRIMARY KEY,
    column_id INTEGER NOT NULL REFERENCES task_columns(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    due TEXT,
    done INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX tasks_column ON tasks (column_id, position);

INSERT INTO task_columns (name, position) VALUES ('To do', 0), ('In progress', 1), ('Done', 2);
