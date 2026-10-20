// Times offered for "Send later" and "Snooze": later today, tomorrow
// morning, the weekend, next week, each with a short label of when.

export interface LaterChoice {
  label: string;
  /** When, as shown next to the label, such as "Wed 8:00". */
  when: string;
  at: Date;
}

function at(day: Date, hour: number): Date {
  return new Date(day.getFullYear(), day.getMonth(), day.getDate(), hour, 0, 0, 0);
}

function plusDays(day: Date, days: number): Date {
  return new Date(day.getFullYear(), day.getMonth(), day.getDate() + days);
}

function short(date: Date, now: Date): string {
  const time = date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  if (date.toDateString() === now.toDateString()) return time;
  return `${date.toLocaleDateString(undefined, { weekday: "short" })} ${time}`;
}

/** The usual choices from `now`. `snooze` adds "Later today" and "This weekend". */
export function laterChoices(now: Date, snooze: boolean): LaterChoice[] {
  const list: [string, Date][] = [];
  const inThree = new Date(now.getTime() + 3 * 3_600_000);
  inThree.setMinutes(0, 0, 0);
  if (snooze && inThree.getDate() === now.getDate() && inThree.getHours() <= 20)
    list.push(["Later today", inThree]);
  const tomorrow = plusDays(now, 1);
  list.push(["Tomorrow morning", at(tomorrow, 8)]);
  if (!snooze) list.push(["Tomorrow afternoon", at(tomorrow, 13)]);
  const day = now.getDay();
  if (snooze && day >= 1 && day <= 4) list.push(["This weekend", at(plusDays(now, 6 - day), 8)]);
  // Next Monday, a week ahead when today is Monday.
  list.push(["Next week", at(plusDays(now, ((8 - day) % 7) || 7), 8)]);
  return list.map(([label, date]) => ({ label, when: short(date, now), at: date }));
}

/** "Wed 21 Oct, 8:00", for toasts and the outbox. */
export function laterText(date: Date): string {
  const now = new Date();
  const time = date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  if (date.toDateString() === now.toDateString()) return `today at ${time}`;
  if (date.toDateString() === plusDays(now, 1).toDateString()) return `tomorrow at ${time}`;
  return `${date.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" })} at ${time}`;
}
