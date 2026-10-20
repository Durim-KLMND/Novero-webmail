// Typed wrappers around the Rust commands in src-tauri/src/commands.rs and compose.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

export type Security = "implicitTls" | "startTls" | "insecure";

export interface Endpoint {
  host: string;
  port: number;
  security: Security;
}

export type OAuthProvider = "google" | "microsoft";

/** How an account signs in: a password, or the provider's own sign-in page. */
export type AuthMethod =
  | { kind: "password" }
  | { kind: "oauth2"; provider: OAuthProvider };

export const providerLabel: Record<OAuthProvider, string> = {
  google: "Google",
  microsoft: "Microsoft",
};

export interface Account {
  id: number;
  displayName: string;
  email: string;
  username: string;
  imap: Endpoint;
  smtp: Endpoint;
  auth: AuthMethod;
  /** Added below new messages, replies and forwards. */
  signature: string;
  /** The signature with formatting; empty when it has none. */
  signatureHtml: string;
}

export type FolderRole =
  | "inbox"
  | "sent"
  | "drafts"
  | "trash"
  | "junk"
  | "archive"
  | "other";

export interface Calendar {
  id: number;
  name: string;
  /** A CSS colour, such as #2e7d6b. */
  color: string;
  account: number | null;
  visible: boolean;
}

export type InviteResponse = "needsAction" | "accepted" | "tentative" | "declined";

export interface Attendee {
  email: string;
  name: string | null;
  response: InviteResponse;
}

/**
 * An event; for a repeating one, its first occurrence and the rule. Times are
 * UTC seconds; an all-day event runs from UTC midnight of its first day to UTC
 * midnight after its last.
 */
export interface CalendarEvent {
  id: number;
  calendar: number;
  uid: string;
  title: string;
  location: string;
  notes: string;
  starts: number;
  ends: number;
  allDay: boolean;
  tz: string | null;
  /** An iCalendar RRULE value, such as FREQ=WEEKLY;BYDAY=MO. */
  rrule: string | null;
  exdates: number[];
  organizer: string | null;
  attendees: Attendee[];
  response: InviteResponse | null;
  /** Minutes before the start to remind, if at all. */
  reminder: number | null;
  sequence: number;
  cancelled: boolean;
}

/** One time an event happens. */
export interface Occurrence {
  event: number;
  calendar: number;
  title: string;
  location: string;
  starts: number;
  ends: number;
  allDay: boolean;
  repeats: boolean;
  cancelled: boolean;
  response: InviteResponse | null;
}

export type RuleField = "from" | "to" | "subject";

export interface RuleCondition {
  field: RuleField;
  /** Holds when the field contains this, ignoring case. */
  text: string;
}

export type RuleAction =
  | { kind: "markRead" }
  | { kind: "flag" }
  | { kind: "move"; folder: number }
  | { kind: "archive" }
  | { kind: "delete" };

/** A filter run on new mail as it arrives in an account's inbox. */
export interface Rule {
  /** 0 until saved. */
  id: number;
  account: number;
  name: string;
  enabled: boolean;
  /** Every condition must hold, rather than any one. */
  matchAll: boolean;
  conditions: RuleCondition[];
  actions: RuleAction[];
}

export interface Folder {
  id: number;
  accountId: number;
  name: string;
  delimiter: string | null;
  role: FolderRole;
  total: number;
  unread: number;
}

export interface Flags {
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  draft: boolean;
  deleted: boolean;
}

/** Which messages the list shows. */
export type Mailbox =
  | { kind: "folder"; id: number }
  | { kind: "allInboxes" }
  | { kind: "search"; query: string };

/** One row of the message list: a message, or a conversation shown by its
 * newest message in the mailbox. */
export interface MessageSummary {
  id: number;
  uid: number;
  accountId: number;
  folderId: number;
  folderName: string;
  folderRole: FolderRole;
  from: string | null;
  to: string | null;
  subject: string | null;
  date: number | null;
  /** Unread while any message of the row is; flagged if any is. */
  flags: Flags;
  hasAttachments: boolean;
  threadId: number | null;
  /** The messages this row stands for, newest first. */
  ids: number[];
  /** Messages in the whole conversation. */
  count: number;
}

export type Theme = "system" | "light" | "dark";

export type UpdateChannel = "stable" | "beta";

export interface Settings {
  theme: Theme;
  conversations: boolean;
  notifications: boolean;
  /** Look for updates in the background and download them. */
  autoUpdate: boolean;
  updateChannel: UpdateChannel;
  /** After a crash, offer to send a report. */
  crashReports: boolean;
}

export const defaultSettings: Settings = {
  theme: "system",
  conversations: true,
  notifications: true,
  autoUpdate: true,
  updateChannel: "stable",
  crashReports: true,
};

/** A newer version, downloaded and ready to install. */
export interface UpdateInfo {
  version: string;
  notes: string | null;
}

export interface MessagePage {
  total: number;
  offset: number;
  items: MessageSummary[];
}

export interface Attachment {
  name: string;
  mimeType: string;
  size: number;
}

export interface ParsedMessage {
  subject: string | null;
  from: string | null;
  to: string | null;
  cc: string | null;
  date: number | null;
  text: string | null;
  html: string | null;
  blockedRemote: number;
  attachments: Attachment[];
}

/** Server settings found for an email address. */
export interface SetupInfo {
  /** The provider's name, or the domain. */
  name: string;
  imap: Endpoint;
  smtp: Endpoint;
  /** The login name when it is not the email address. */
  username: string | null;
  /** The provider's own sign-in page, if it has one. */
  oauth: OAuthProvider | null;
  /** Whether this build can open that sign-in page. */
  oauthAvailable: boolean;
  /** What to know before signing in with a password. */
  note: string | null;
  source: "known" | "autoconfig" | "dns" | "guess";
}

export interface NewAccountInput {
  displayName: string;
  email: string;
  username: string;
  password: string;
  imap: Endpoint;
  smtp: Endpoint;
  auth: AuthMethod;
}

export type FlagKind = "seen" | "flagged" | "answered";

/** What the user edits in the composer. Addresses are comma separated. */
export interface DraftFields {
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  /** The text as typed; with formatting, the plain-text copy of `html`. */
  body: string;
  /** The formatted text; empty for plain-text messages. */
  html: string;
}

export interface Draft extends DraftFields {
  id: number;
  account: number;
  messageId: string;
  attachments: Attachment[];
  inReplyTo: string | null;
  references: string[];
  replyingTo: number | null;
}

/** A known address, suggested while typing one. */
export interface Contact {
  email: string;
  name: string | null;
}

export type OutboxState = "queued" | "failed" | "sent";

export interface OutboxItem {
  id: number;
  accountId: number;
  messageId: string;
  subject: string;
  recipients: string[];
  state: OutboxState;
  attempts: number;
  lastError: string | null;
  createdAt: number;
}

/** "messages": the message lists of every folder; "bodies": recent messages
 * downloaded in full so search covers their text. */
export type SyncStage = "messages" | "bodies";

export type MailEvent =
  | { type: "syncStarted"; account: number }
  | {
      type: "syncProgress";
      account: number;
      folder: string;
      done: number;
      total: number;
    }
  | {
      type: "accountProgress";
      account: number;
      stage: SyncStage;
      done: number;
      total: number;
    }
  | { type: "syncFinished"; account: number; error: string | null }
  | {
      type: "pendingChanges";
      account: number;
      pending: number;
      error: string | null;
    }
  | { type: "mailboxChanged"; account: number }
  | {
      type: "outboxChanged";
      account: number;
      queued: number;
      failed: number;
      error: string | null;
    };

export const api = {
  discoverSettings: (email: string) =>
    invoke<SetupInfo | null>("discover_settings", { email }),
  addAccount: (input: NewAccountInput) =>
    invoke<Account>("add_account", { input }),
  signInAgain: (id: number, password: string | null) =>
    invoke<Account>("sign_in_again", { id, password }),
  cancelSignIn: () => invoke<void>("cancel_sign_in"),
  listAccounts: () => invoke<Account[]>("list_accounts"),
  removeAccount: (id: number) => invoke<void>("remove_account", { id }),
  listFolders: (account: number) =>
    invoke<Folder[]>("list_folders", { account }),
  listMessages: (mailbox: Mailbox, offset: number, limit: number) =>
    invoke<MessagePage>("list_messages", { mailbox, offset, limit }),
  conversation: (message: number) =>
    invoke<MessageSummary[]>("conversation", { message }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) =>
    invoke<Settings>("save_settings", { settings }),
  listRules: (account: number) => invoke<Rule[]>("list_rules", { account }),
  saveRule: (rule: Rule) => invoke<Rule>("save_rule", { rule }),
  deleteRule: (id: number) => invoke<void>("delete_rule", { id }),
  orderRules: (account: number, ids: number[]) =>
    invoke<void>("order_rules", { account, ids }),
  setSignature: (account: number, signature: string, html: string) =>
    invoke<void>("set_signature", { account, signature, html }),
  openMessage: (id: number, allowRemote: boolean) =>
    invoke<ParsedMessage>("open_message", { id, allowRemote }),
  syncAccount: (account: number) => invoke<void>("sync_account", { account }),
  pendingChanges: (account: number) =>
    invoke<number>("pending_changes", { account }),
  setFlag: (ids: number[], flag: FlagKind, value: boolean) =>
    invoke<void>("set_flag", { ids, flag, value }),
  moveMessages: (ids: number[], folder: number) =>
    invoke<void>("move_messages", { ids, folder }),
  archiveMessages: (ids: number[]) => invoke<void>("archive_messages", { ids }),
  deleteMessages: (ids: number[]) => invoke<void>("delete_messages", { ids }),
  newDraft: (account: number) => invoke<Draft>("new_draft", { account }),
  replyDraft: (message: number, all: boolean) =>
    invoke<Draft>("reply_draft", { message, all }),
  forwardDraft: (message: number) =>
    invoke<Draft>("forward_draft", { message }),
  openDraft: (message: number) => invoke<Draft>("open_draft", { message }),
  getDraft: (id: number) => invoke<Draft>("get_draft", { id }),
  /** Opens a draft in a window of its own. */
  openComposerWindow: (id: number) => invoke<void>("open_composer_window", { id }),
  saveDraft: (id: number, fields: DraftFields, upload: boolean) =>
    invoke<Draft>("save_draft", { id, fields, upload }),
  attachFiles: (id: number) => invoke<Draft>("attach_files", { id }),
  attachDropped: (id: number, drop: number, only: number[] | null) =>
    invoke<Draft>("attach_dropped", { id, drop, only }),
  droppedPicture: (drop: number, index: number) =>
    invoke<string>("dropped_picture", { drop, index }),
  pickPicture: () => invoke<string | null>("pick_picture"),
  removeAttachment: (id: number, index: number) =>
    invoke<Draft>("remove_attachment", { id, index }),
  discardDraft: (id: number) => invoke<void>("discard_draft", { id }),
  sendDraft: (id: number, fields: DraftFields) =>
    invoke<void>("send_draft", { id, fields }),
  suggestContacts: (query: string) =>
    invoke<Contact[]>("suggest_contacts", { query }),
  hideContact: (email: string) => invoke<void>("hide_contact", { email }),
  listOutbox: (account: number) =>
    invoke<OutboxItem[]>("list_outbox", { account }),
  outboxStatus: (account: number) =>
    invoke<[number, number]>("outbox_status", { account }),
  retryOutbox: (id: number) => invoke<void>("retry_outbox", { id }),
  deleteOutbox: (id: number) => invoke<void>("delete_outbox", { id }),
  editOutbox: (id: number) => invoke<Draft>("edit_outbox", { id }),
  appVersion: () => invoke<string>("app_version"),
  mailtoDraft: (account: number, link: string) =>
    invoke<Draft>("mailto_draft", { account, link }),
  takeMailto: () => invoke<string | null>("take_mailto"),
  openDefaultApps: () => invoke<void>("open_default_apps"),
  crashReport: () => invoke<string | null>("crash_report"),
  discardCrashReport: () => invoke<void>("discard_crash_report"),
  sendCrashReport: () => invoke<void>("send_crash_report"),
  updatesAvailable: () => invoke<boolean>("updates_available"),
  checkForUpdate: (channel: UpdateChannel) =>
    invoke<UpdateInfo | null>("check_for_update", { channel }),
  readyUpdate: () => invoke<UpdateInfo | null>("ready_update"),
  installUpdate: () => invoke<void>("install_update"),
  listCalendars: () => invoke<Calendar[]>("list_calendars"),
  saveCalendar: (calendar: Calendar) => invoke<Calendar>("save_calendar", { calendar }),
  deleteCalendar: (id: number) => invoke<void>("delete_calendar", { id }),
  calendarOccurrences: (from: number, to: number) =>
    invoke<Occurrence[]>("calendar_occurrences", { from, to }),
  getEvent: (id: number) => invoke<CalendarEvent | null>("get_event", { id }),
  saveEvent: (event: CalendarEvent) => invoke<CalendarEvent>("save_event", { event }),
  deleteEvent: (id: number) => invoke<void>("delete_event", { id }),
  skipOccurrence: (id: number, starts: number) =>
    invoke<CalendarEvent>("skip_occurrence", { id, starts }),
};

/** Calendars or events changed, in this window or another. */
export function onCalendarChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("calendar-changed", () => handler());
}

/** Files dragged over the window (positions in CSS pixels), and dropped. */
export type FileDrop =
  | { kind: "over"; x: number; y: number }
  | { kind: "leave" }
  | { kind: "drop"; id: number; x: number; y: number; files: { name: string; picture: boolean }[] };

export function onFileDrop(handler: (event: FileDrop) => void): Promise<UnlistenFn> {
  // Drops are sent to the window they landed on.
  return getCurrentWebviewWindow().listen<FileDrop>("file-drop", (e) => handler(e.payload));
}

/** A downloaded update is ready to install. */
export function onUpdateReady(handler: (update: UpdateInfo) => void): Promise<UnlistenFn> {
  return listen<UpdateInfo>("update-ready", (e) => handler(e.payload));
}

/** A mailto: link was opened; `api.takeMailto` returns it. */
export function onMailto(handler: () => void): Promise<UnlistenFn> {
  return listen("mailto", () => handler());
}

export function onMailEvent(
  handler: (event: MailEvent) => void,
): Promise<UnlistenFn> {
  return listen<MailEvent>("mail-event", (e) => handler(e.payload));
}

/** Errors from commands arrive as plain strings. */
export function errorText(err: unknown): string {
  if (typeof err === "string")
    return err.charAt(0).toUpperCase() + err.slice(1);
  if (err instanceof Error) return err.message;
  return "Something went wrong.";
}
