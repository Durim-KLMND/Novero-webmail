const timeFmt = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const dayFmt = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short" });
const yearFmt = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", year: "numeric" });
const fullFmt = new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "short" });

/** Short date for the message list: time today, day this year, full date otherwise. */
export function listDate(seconds: number | null): string {
  if (seconds == null) return "";
  const d = new Date(seconds * 1000);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return timeFmt.format(d);
  if (d.getFullYear() === now.getFullYear()) return dayFmt.format(d);
  return yearFmt.format(d);
}

export function fullDate(seconds: number | null): string {
  return seconds == null ? "" : fullFmt.format(new Date(seconds * 1000));
}

/** "Jane Doe <jane@x.test>" becomes "Jane Doe"; a bare address stays as it is. */
export function senderName(from: string | null): string {
  if (!from) return "(no sender)";
  const match = from.match(/^\s*"?([^"<]*?)"?\s*<[^>]+>\s*$/);
  return match && match[1] ? match[1] : from;
}

/** The address of a From field, in lower case: "jane@example.com". */
export function senderEmail(from: string | null): string {
  if (!from) return "";
  const match = from.match(/<([^>]+)>/);
  return (match ? match[1] : from).trim().toLowerCase();
}

/** "To Jane Doe +2" for the first of a list of recipients. */
export function recipientLabel(to: string | null): string {
  if (!to) return "(no recipients)";
  // Split on commas outside quoted names.
  const all = to.split(/,(?=(?:[^"]*"[^"]*")*[^"]*$)/).map((s) => s.trim()).filter(Boolean);
  const more = all.length > 1 ? ` +${all.length - 1}` : "";
  return `To ${senderName(all[0] ?? to)}${more}`;
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

const folderNames: Record<string, string> = {
  inbox: "Inbox",
  sent: "Sent",
  drafts: "Drafts",
  trash: "Trash",
  junk: "Junk",
  archive: "Archive",
};

/** Display name of a folder: friendly names for special folders, the last path part otherwise. */
export function folderLabel(name: string, role: string, delimiter: string | null): string {
  if (folderNames[role]) return folderNames[role];
  return decodeFolderName(delimiter ? name.split(delimiter).pop() || name : name);
}

/** A folder's whole path, e.g. "Projects › 2026", for tooltips and menus. */
export function folderPath(name: string, delimiter: string | null): string {
  return decodeFolderName(delimiter ? name.split(delimiter).join(" › ") : name);
}

/** Server folder names write other letters in "modified UTF-7" (RFC 3501):
 *  "Entw&APw-rfe" is "Entwürfe". Anything malformed is shown as it is. */
export function decodeFolderName(name: string): string {
  return name.replace(/&([^-]*)-/g, (whole, run: string) => {
    if (!run) return "&";
    try {
      const bytes = atob(run.replace(/,/g, "/") + "===".slice((run.length + 3) % 4));
      let text = "";
      for (let i = 0; i + 1 < bytes.length; i += 2) {
        text += String.fromCharCode((bytes.charCodeAt(i) << 8) | bytes.charCodeAt(i + 1));
      }
      return text;
    } catch {
      return whole;
    }
  });
}
