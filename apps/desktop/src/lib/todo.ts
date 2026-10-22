// Small helpers for the To-do view: due days, and how notes are listed.
import type { Board, Note, Task } from "./api";

/** A day as YYYY-MM-DD in local time, as a date input gives it. */
export function dayKey(date: Date): string {
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${m}-${d}`;
}

/** Unfinished tasks due today or earlier: the number on the rail. */
export function dueCount(board: Board, today = dayKey(new Date())): number {
  return board.tasks.filter((t) => !t.done && t.due != null && t.due <= today).length;
}

export type DueState = "overdue" | "today" | "soon" | "later";

export function dueState(task: Task, today = new Date()): DueState | null {
  if (!task.due) return null;
  const key = dayKey(today);
  if (task.due < key) return "overdue";
  if (task.due === key) return "today";
  const inAWeek = new Date(today);
  inAWeek.setDate(inAWeek.getDate() + 7);
  return task.due <= dayKey(inAWeek) ? "soon" : "later";
}

/** "Today", "Tomorrow", "Yesterday", a weekday this week, or a date. */
export function dueText(day: string, today = new Date()): string {
  const [y, m, d] = day.split("-").map(Number);
  const date = new Date(y, m - 1, d);
  const start = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  const days = Math.round((date.getTime() - start.getTime()) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Tomorrow";
  if (days === -1) return "Yesterday";
  if (days > 1 && days < 7) return date.toLocaleDateString(undefined, { weekday: "long" });
  return date.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: date.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}

/** What a note is called in the list: its title, or its first line. */
export function noteTitle(note: Note): string {
  return note.title.trim() || note.body.trim().split("\n")[0].slice(0, 80) || "New note";
}

/** The text under a note's title in the list. */
export function notePreview(note: Note): string {
  const lines = note.body.trim().split("\n").map((l) => l.trim()).filter(Boolean);
  return (note.title.trim() ? lines : lines.slice(1)).join(" ").slice(0, 140);
}

/** Notes whose title or text holds every word of `query`. */
export function matchNotes(notes: Note[], query: string): Note[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return notes;
  return notes.filter((n) => {
    const text = `${n.title}\n${n.body}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

/** When a note was changed, short: a time today, else a date. */
export function editedText(seconds: number, now = new Date()): string {
  const date = new Date(seconds * 1000);
  if (dayKey(date) === dayKey(now)) return date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  return date.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}
