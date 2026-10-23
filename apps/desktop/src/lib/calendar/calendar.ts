// Date math for the calendar view. Event times are UTC seconds; the view works
// in local Dates. An all-day event is stored from UTC midnight to UTC
// midnight, so its dates are read from the UTC parts and shown as local days.
import type { Occurrence } from "../api";

export const DAY_MS = 86_400_000;

export type CalendarMode = "month" | "week" | "day";

export function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

/** Adds days by the calendar, so a day across a clock change stays one day. */
export function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days, date.getHours(), date.getMinutes());
}

/** Weeks start on Monday. */
export function startOfWeek(date: Date): Date {
  const day = startOfDay(date);
  return addDays(day, -((day.getDay() + 6) % 7));
}

export function startOfMonth(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), 1);
}

export function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

export const toSecs = (date: Date) => Math.floor(date.getTime() / 1000);
export const fromSecs = (secs: number) => new Date(secs * 1000);

/** The local day an all-day time (UTC midnight) stands for. */
export function allDayDate(secs: number): Date {
  const utc = new Date(secs * 1000);
  return new Date(utc.getUTCFullYear(), utc.getUTCMonth(), utc.getUTCDate());
}

/** The all-day time (UTC midnight) of a local day. */
export function allDaySecs(date: Date): number {
  return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 1000;
}

/** Where an occurrence sits in local time: [start, end). */
export function span(o: Occurrence): [Date, Date] {
  return o.allDay ? [allDayDate(o.starts), allDayDate(o.ends)] : [fromSecs(o.starts), fromSecs(o.ends)];
}

/** Whether an occurrence touches the day starting at `day`. */
export function onDay(o: Occurrence, day: Date): boolean {
  const [start, end] = span(o);
  const next = addDays(day, 1);
  // A zero-length event still shows on its day.
  return start < next && (end > day || (+end === +start && start >= day));
}

/** The days a view shows, first to last. */
export function viewDays(mode: CalendarMode, anchor: Date): Date[] {
  const first =
    mode === "month" ? startOfWeek(startOfMonth(anchor)) : mode === "week" ? startOfWeek(anchor) : startOfDay(anchor);
  const count = mode === "month" ? 42 : mode === "week" ? 7 : 1;
  return Array.from({ length: count }, (_, i) => addDays(first, i));
}

/** The date one step before or after, by the view's unit. */
export function step(mode: CalendarMode, anchor: Date, by: number): Date {
  if (mode === "month") return new Date(anchor.getFullYear(), anchor.getMonth() + by, 1);
  return addDays(anchor, by * (mode === "week" ? 7 : 1));
}

export function viewTitle(mode: CalendarMode, anchor: Date): string {
  if (mode === "month") return anchor.toLocaleDateString(undefined, { month: "long", year: "numeric" });
  if (mode === "day")
    return anchor.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long", year: "numeric" });
  const days = viewDays("week", anchor);
  const [a, b] = [days[0], days[6]];
  if (a.getMonth() === b.getMonth())
    return `${a.getDate()}–${b.getDate()} ${b.toLocaleDateString(undefined, { month: "long", year: "numeric" })}`;
  const short = (d: Date) => d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  return `${short(a)} – ${short(b)} ${b.getFullYear()}`;
}

export function timeText(date: Date): string {
  return date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** "2026-10-20" for a date input. */
export function dateValue(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** "09:30" for a time input. */
export function timeValue(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** The local Date of a date input's and a time input's values. */
export function parseLocal(date: string, time = "00:00"): Date | null {
  const d = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  const t = /^(\d{1,2}):(\d{2})/.exec(time);
  if (!d || !t) return null;
  return new Date(+d[1], +d[2] - 1, +d[3], +t[1], +t[2]);
}

/**
 * Lays out the timed events of one day side by side where they overlap: each
 * gets a column and the number of columns in its group.
 */
export function columns<T extends { start: number; end: number }>(items: T[]): (T & { col: number; cols: number })[] {
  const sorted = [...items].sort((a, b) => a.start - b.start || b.end - a.end);
  const placed: (T & { col: number; cols: number })[] = [];
  let group: (T & { col: number; cols: number })[] = [];
  let groupEnd = -Infinity;
  const close = () => {
    const cols = Math.max(1, ...group.map((g) => g.col + 1));
    for (const g of group) g.cols = cols;
  };
  for (const item of sorted) {
    if (item.start >= groupEnd) {
      close();
      group = [];
      groupEnd = -Infinity;
    }
    const ends: number[] = [];
    for (const g of group) ends[g.col] = Math.max(ends[g.col] ?? -Infinity, g.end);
    let col = ends.findIndex((e) => e <= item.start);
    if (col < 0) col = ends.length;
    const entry = { ...item, col, cols: 1 };
    group.push(entry);
    placed.push(entry);
    groupEnd = Math.max(groupEnd, item.end);
  }
  close();
  return placed;
}

const WEEKDAYS = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

export type Repeat = "none" | "daily" | "weekdays" | "weekly" | "monthly" | "yearly" | "custom";

/** The repeat rule for a choice, starting on `date`. */
export function repeatRule(repeat: Repeat, date: Date, custom: string | null): string | null {
  switch (repeat) {
    case "none":
      return null;
    case "daily":
      return "FREQ=DAILY";
    case "weekdays":
      return "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR";
    case "weekly":
      return `FREQ=WEEKLY;BYDAY=${WEEKDAYS[date.getDay()]}`;
    case "monthly":
      return `FREQ=MONTHLY;BYMONTHDAY=${date.getDate()}`;
    case "yearly":
      return "FREQ=YEARLY";
    case "custom":
      return custom;
  }
}

/** The choice a stored rule matches, or "custom" for anything else. */
export function repeatOf(rule: string | null, date: Date): Repeat {
  if (!rule) return "none";
  for (const r of ["daily", "weekdays", "weekly", "monthly", "yearly"] as Repeat[])
    if (repeatRule(r, date, null) === rule) return r;
  if (rule === "FREQ=WEEKLY") return "weekly";
  if (rule === "FREQ=MONTHLY") return "monthly";
  return "custom";
}

/** "Weekly on Tuesday" and the like, for the repeat menu. */
export function repeatLabel(repeat: Repeat, date: Date): string {
  const weekday = date.toLocaleDateString(undefined, { weekday: "long" });
  switch (repeat) {
    case "none":
      return "Doesn't repeat";
    case "daily":
      return "Every day";
    case "weekdays":
      return "Every weekday (Monday to Friday)";
    case "weekly":
      return `Every week on ${weekday}`;
    case "monthly":
      return `Every month on day ${date.getDate()}`;
    case "yearly":
      return `Every year on ${date.toLocaleDateString(undefined, { day: "numeric", month: "long" })}`;
    case "custom":
      return "Custom (set in the invitation)";
  }
}

export const REMINDERS: [number | null, string][] = [
  [null, "No reminder"],
  [0, "When it starts"],
  [5, "5 minutes before"],
  [10, "10 minutes before"],
  [15, "15 minutes before"],
  [30, "30 minutes before"],
  [60, "1 hour before"],
  [120, "2 hours before"],
  [1440, "1 day before"],
  [10080, "1 week before"],
];

/** Colours offered for calendars. */
export const COLORS: [string, string][] = [
  ["#2e7d6b", "Teal"],
  ["#1f6fb2", "Blue"],
  ["#7b4fc9", "Purple"],
  ["#c2416c", "Pink"],
  ["#d9622b", "Orange"],
  ["#b8860b", "Gold"],
  ["#5b7f2a", "Green"],
  ["#6b6b66", "Grey"],
];
