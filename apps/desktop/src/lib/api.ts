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
  /** Added below new messages. */
  signature: string;
  /** The signature with formatting; empty when it has none. */
  signatureHtml: string;
  /** Added to replies and forwards, plain and formatted. */
  replySignature: string;
  replySignatureHtml: string;
  /** Which signatures new messages and replies start with; null for none. */
  signatureNew: number | null;
  signatureReply: number | null;
}

/** One of an account's signatures; id 0 until it is saved. */
export interface Signature {
  id: number;
  account: number;
  name: string;
  text: string;
  /** With formatting; empty when it has none. */
  html: string;
}

/** A saved message to start new ones from; id 0 until it is saved. */
export interface Template {
  id: number;
  name: string;
  subject: string;
  text: string;
  html: string;
}

/** A note in the To-do view. `id` is 0 for one not saved yet. */
export interface Note {
  id: number;
  title: string;
  body: string;
  pinned: boolean;
  /** When the title or text last changed, in seconds since 1970. */
  updated: number;
}

export interface TaskColumn {
  id: number;
  name: string;
}

/** A card on the task board. `id` is 0 for one not saved yet. */
export interface Task {
  id: number;
  column: number;
  title: string;
  notes: string;
  /** YYYY-MM-DD, or null. */
  due: string | null;
  done: boolean;
}

/** Columns left to right; cards top to bottom within their column. */
export interface Board {
  columns: TaskColumn[];
  tasks: Task[];
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
  /** The online account it syncs with; null when kept on this computer. */
  source?: number | null;
  /** An online calendar that can only be looked at. */
  readOnly?: boolean;
}

/** A Google or Microsoft account whose calendars sync. */
export interface CalendarAccount {
  id: number;
  provider: OAuthProvider;
  email: string;
  /** When the last sync finished, in UTC seconds. */
  syncedAt: number | null;
  error: string | null;
  needsSignIn: boolean;
}

export interface CalendarAccounts {
  accounts: CalendarAccount[];
  syncing: boolean;
  /** Providers this build can sign in with. */
  providers: OAuthProvider[];
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
  /** For one occurrence of an online repeating event: its series. */
  series?: string | null;
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
/** How a composer ended. */
export type ComposerOutcome = "sent" | "scheduled" | "saved" | "discarded";

/** What kind of mail a message is, for mail cleanup. */
export type Category =
  | "personal"
  | "updates"
  | "social"
  | "forums"
  | "newsletters"
  | "promotions"
  | "spam";

/** Why a category was suggested (see `mail_core::categories::Reason`). */
export type CategoryReason =
  | "spamFilter"
  | "spamWords"
  | "nameMismatch"
  | "contact"
  | "replied"
  | "ownAddress"
  | "personLike"
  | "socialSite"
  | "discussion"
  | "mailingList"
  | "serviceSite"
  | "automated"
  | "noReply"
  | "updateWords"
  | "promoWords"
  | "promoSender"
  | "newsletterSender"
  | "newsWords"
  | "unsubscribe"
  | "bulkMail";

/** Every inbox, or the inbox, Archive and the user's own folders. */
export type CleanupScope = "inbox" | "allMail";

/** One sender's mail of one category. */
export interface SenderGroup {
  /** Lower case; empty when the mail has no sender. */
  email: string;
  name: string | null;
  category: Category;
  /** The user chose this category, for the sender or each message. */
  confirmed: boolean;
  senderCategory: Category | null;
  sure: boolean;
  reasons: CategoryReason[];
  /** Newest first. */
  ids: number[];
  unread: number;
  latest: number | null;
  subject: string | null;
  accounts: number[];
  /** An https or mailto link to leave the list. */
  unsubscribe: string | null;
}

export interface CleanupReport {
  groups: SenderGroup[];
  /** Messages whose list headers haven't been read yet. */
  unchecked: number;
}

export interface CleanupItem {
  id: number;
  accountId: number;
  subject: string | null;
  date: number | null;
  unread: boolean;
  category: Category;
  confirmed: boolean;
  sure: boolean;
  reasons: CategoryReason[];
}

export type Mailbox =
  | { kind: "folder"; id: number }
  | { kind: "allInboxes" }
  | { kind: "snoozed" }
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
  /** An inbox row's cleanup category, when labels are on. */
  category: Category | null;
}

export type Theme = "system" | "light" | "dark";

export interface Settings {
  theme: Theme;
  conversations: boolean;
  /** Inbox rows show their cleanup category. */
  categoryLabels: boolean;
  notifications: boolean;
  /** Look for updates in the background and download them. */
  autoUpdate: boolean;
  /** After a crash, offer to send a report. */
  crashReports: boolean;
  /** Draw Novero Mail's own title bar instead of the Windows one. */
  customTitlebar: boolean;
  /** The close button hides the main window to the system tray instead of quitting. */
  closeToTray: boolean;
  /** Show the unread inbox count on the taskbar icon. */
  unreadBadge: boolean;
}

export const defaultSettings: Settings = {
  theme: "system",
  conversations: true,
  categoryLabels: true,
  notifications: true,
  autoUpdate: true,
  crashReports: true,
  customTitlebar: true,
  closeToTray: true,
  unreadBadge: true,
};

/** A newer version. */
export interface UpdateInfo {
  version: string;
  /** The release notes, as written on GitHub (see notes.ts). */
  notes: string | null;
  /** When it was released, in seconds since 1970, when known. */
  released: number | null;
}

export type UpdatePhase = "idle" | "checking" | "available" | "downloading" | "ready" | "installing";

/** Where updates are at (src-tauri/src/updates.rs). */
export interface UpdateStatus {
  phase: UpdatePhase;
  /** The version found, from "available" on. */
  update: UpdateInfo | null;
  downloaded: number;
  total: number | null;
  /** When a check last reached the server, in seconds since 1970. */
  lastChecked: number | null;
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
  /** Remote images were let through: asked for, or a trusted sender. */
  imagesAllowed: boolean;
  attachments: Attachment[];
  /** Carries a calendar invitation; `api.messageInvite` reads it. */
  invite: boolean;
}

/** A calendar invitation in a message. */
export interface InviteView {
  method: "request" | "reply" | "cancel" | "publish";
  /** The event as the message describes it. */
  event: CalendarEvent;
  /** The event as the calendar has it, if it is there. */
  saved: CalendarEvent | null;
  /** For a reply: who answered, and how. */
  answer: Attendee | null;
  /** An invitation from someone else, which can be accepted or declined. */
  canAnswer: boolean;
  /** Titles of other events at the same time. */
  conflicts: string[];
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

/** New servers for an account; username and password apply to password accounts. */
export interface ServerSettingsInput {
  imap: Endpoint;
  smtp: Endpoint;
  username: string;
  /** Empty to keep the saved password. */
  password: string;
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
  /** When a scheduled message goes (UTC seconds). */
  sendAt: number | null;
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
      /** Scheduled messages waiting for their time. */
      scheduled: number;
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
  updateAccountServers: (id: number, input: ServerSettingsInput) =>
    invoke<Account>("update_account_servers", { id, input }),
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
  /** Addresses and domains whose remote images always load. */
  listImageSenders: () => invoke<string[]>("list_image_senders"),
  allowImagesFrom: (sender: string) => invoke<string[]>("allow_images_from", { sender }),
  forgetImageSender: (sender: string) => invoke<string[]>("forget_image_sender", { sender }),
  listRules: (account: number) => invoke<Rule[]>("list_rules", { account }),
  saveRule: (rule: Rule) => invoke<Rule>("save_rule", { rule }),
  deleteRule: (id: number) => invoke<void>("delete_rule", { id }),
  orderRules: (account: number, ids: number[]) =>
    invoke<void>("order_rules", { account, ids }),
  /** Runs one rule, or every rule that is on, on the mail in a folder;
   *  returns how many messages matched. */
  runRules: (folder: number, rule: number | null) => invoke<number>("run_rules", { folder, rule }),
  listSignatures: () => invoke<Signature[]>("list_signatures"),
  saveSignature: (signature: Signature) => invoke<Signature>("save_signature", { signature }),
  deleteSignature: (id: number) => invoke<void>("delete_signature", { id }),
  setDefaultSignatures: (account: number, newMessages: number | null, replies: number | null) =>
    invoke<void>("set_default_signatures", { account, new: newMessages, reply: replies }),
  listTemplates: () => invoke<Template[]>("list_templates"),
  saveTemplate: (template: Template) => invoke<Template>("save_template", { template }),
  deleteTemplate: (id: number) => invoke<void>("delete_template", { id }),
  listNotes: () => invoke<Note[]>("list_notes"),
  saveNote: (note: Note) => invoke<Note>("save_note", { note }),
  deleteNote: (id: number) => invoke<void>("delete_note", { id }),
  taskBoard: () => invoke<Board>("task_board"),
  saveTaskColumn: (column: TaskColumn) => invoke<TaskColumn>("save_task_column", { column }),
  deleteTaskColumn: (id: number) => invoke<void>("delete_task_column", { id }),
  moveTaskColumn: (id: number, index: number) => invoke<void>("move_task_column", { id, index }),
  saveTask: (task: Task) => invoke<Task>("save_task", { task }),
  /** Puts the card in `column` at `index` (0 is the top). */
  moveTask: (id: number, column: number, index: number) =>
    invoke<void>("move_task", { id, column, index }),
  deleteTask: (id: number) => invoke<void>("delete_task", { id }),
  clearDoneTasks: (column: number) => invoke<void>("clear_done_tasks", { column }),
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
  junkMessages: (ids: number[]) => invoke<void>("junk_messages", { ids }),
  /** Every sender's mail in a scope, grouped by sender and category. */
  /** Accounts with category folders on their server. */
  categoryFolderAccounts: () => invoke<number[]>("category_folder_accounts"),
  makeCategoryFolders: (accounts: number[]) =>
    invoke<{ filed: number; failed: string[] }>("make_category_folders", { accounts }),
  fileIntoCategoryFolders: () => invoke<number>("file_into_category_folders"),
  stopCategoryFolders: () => invoke<void>("stop_category_folders"),
  cleanupReport: (scope: CleanupScope, account: number | null) =>
    invoke<CleanupReport>("cleanup_report", { scope, account }),
  cleanupItems: (ids: number[]) => invoke<CleanupItem[]>("cleanup_items", { ids }),
  /** Puts everything from a sender in a category; null goes back to guessing. */
  setSenderCategory: (email: string, category: Category | null) =>
    invoke<void>("set_sender_category", { email, category }),
  setMessageCategory: (ids: number[], category: Category | null) =>
    invoke<void>("set_message_category", { ids, category }),
  /** Reads list headers older mail was cached without; returns how many are still unread. */
  checkListHeaders: (scope: CleanupScope, account: number | null) =>
    invoke<number>("check_list_headers", { scope, account }),
  /** Makes a folder inside `parent`, or at the top when it is null. */
  createFolder: (account: number, parent: number | null, name: string) =>
    invoke<Folder>("create_folder", { account, parent, name }),
  renameFolder: (folder: number, name: string) => invoke<void>("rename_folder", { folder, name }),
  /** Deletes a folder with its subfolders and their mail, on the server too. */
  deleteFolder: (folder: number) => invoke<void>("delete_folder", { folder }),
  markFolderRead: (folder: number) => invoke<void>("mark_folder_read", { folder }),
  /** Deletes everything in Trash or Junk for good. */
  emptyFolder: (folder: number) => invoke<void>("empty_folder", { folder }),
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
  /** Sends now, or at `sendAt` (UTC seconds). */
  sendDraft: (id: number, fields: DraftFields, sendAt: number | null = null) =>
    invoke<void>("send_draft", { id, fields, sendAt }),
  sendOutboxNow: (id: number) => invoke<void>("send_outbox_now", { id }),
  snoozeMessages: (ids: number[], until: number) => invoke<void>("snooze_messages", { ids, until }),
  unsnoozeMessages: (ids: number[]) => invoke<void>("unsnooze_messages", { ids }),
  snoozedCount: () => invoke<number>("snoozed_count"),
  snoozedUntil: (id: number) => invoke<number | null>("snoozed_until", { id }),
  suggestContacts: (query: string) =>
    invoke<Contact[]>("suggest_contacts", { query }),
  hideContact: (email: string) => invoke<void>("hide_contact", { email }),
  listOutbox: (account: number) =>
    invoke<OutboxItem[]>("list_outbox", { account }),
  outboxStatus: (account: number) =>
    invoke<[number, number, number]>("outbox_status", { account }),
  retryOutbox: (id: number) => invoke<void>("retry_outbox", { id }),
  deleteOutbox: (id: number) => invoke<void>("delete_outbox", { id }),
  editOutbox: (id: number) => invoke<Draft>("edit_outbox", { id }),
  appVersion: () => invoke<string>("app_version"),
  mailtoDraft: (account: number, link: string) =>
    invoke<Draft>("mailto_draft", { account, link }),
  takeMailto: () => invoke<string | null>("take_mailto"),
  openDefaultApps: () => invoke<void>("open_default_apps"),
  /** Shows a sample new-mail notification after `delay` seconds. */
  testNotification: (several: boolean, delay: number) =>
    invoke<void>("test_notification", { several, delay }),
  openNotificationSettings: () => invoke<void>("open_notification_settings"),
  crashReport: () => invoke<string | null>("crash_report"),
  discardCrashReport: () => invoke<void>("discard_crash_report"),
  sendCrashReport: () => invoke<void>("send_crash_report"),
  updatesAvailable: () => invoke<boolean>("updates_available"),
  checkForUpdate: () => invoke<UpdateInfo | null>("check_for_update"),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  downloadUpdate: () => invoke<void>("download_update"),
  installUpdate: () => invoke<void>("install_update"),
  listCalendars: () => invoke<Calendar[]>("list_calendars"),
  saveCalendar: (calendar: Calendar) => invoke<Calendar>("save_calendar", { calendar }),
  deleteCalendar: (id: number) => invoke<void>("delete_calendar", { id }),
  calendarOccurrences: (from: number, to: number) =>
    invoke<Occurrence[]>("calendar_occurrences", { from, to }),
  getEvent: (id: number) => invoke<CalendarEvent | null>("get_event", { id }),
  saveEvent: (event: CalendarEvent) => invoke<CalendarEvent>("save_event", { event }),
  deleteEvent: (id: number) => invoke<void>("delete_event", { id }),
  /** The decoded contents of a received attachment (`index` into `ParsedMessage.attachments`). */
  attachmentContents: (id: number, index: number) =>
    invoke<ArrayBuffer>("attachment_contents", { id, index }),
  /** Asks where to save an attachment; false when cancelled. */
  saveAttachment: (id: number, index: number) => invoke<boolean>("save_attachment", { id, index }),
  messageInvite: (id: number) => invoke<InviteView | null>("message_invite", { id }),
  answerInvite: (id: number, response: InviteResponse, send: boolean) =>
    invoke<CalendarEvent>("answer_invite", { id, response, send }),
  /** Null when the occurrence was its own event and is now gone. */
  skipOccurrence: (id: number, starts: number) =>
    invoke<CalendarEvent | null>("skip_occurrence", { id, starts }),
  listCalendarAccounts: () => invoke<CalendarAccounts>("list_calendar_accounts"),
  /** Signs in for an account's calendars; with `id`, that account again. */
  connectCalendar: (provider: OAuthProvider, id: number | null = null) =>
    invoke<CalendarAccount>("connect_calendar", { provider, id }),
  disconnectCalendar: (id: number) => invoke<void>("disconnect_calendar", { id }),
  syncCalendars: () => invoke<void>("sync_calendars"),
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

/** Updates were looked for, found, downloaded or installed. */
export function onUpdateStatus(handler: (status: UpdateStatus) => void): Promise<UnlistenFn> {
  return listen<UpdateStatus>("update-status", (e) => handler(e.payload));
}

/** A mailto: link was opened; `api.takeMailto` returns it. */
export function onMailto(handler: () => void): Promise<UnlistenFn> {
  return listen("mailto", () => handler());
}

/** Reading list headers for mail cleanup: how far along it is. */
export function onCleanupProgress(
  handler: (progress: { done: number; total: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ done: number; total: number }>("cleanup-progress", (e) => handler(e.payload));
}

export function onMailEvent(
  handler: (event: MailEvent) => void,
): Promise<UnlistenFn> {
  return listen<MailEvent>("mail-event", (e) => handler(e.payload));
}

/** A dropped connection is expected and recovers by itself, so it is not shown as an error. */
export function isOffline(error: string | null | undefined): boolean {
  return !!error && /^network error/i.test(error);
}

/** The server turned down the password or sign-in. */
export function isSignInError(error: string | null | undefined): boolean {
  return !!error && /^authentication failed/i.test(error);
}

/** Errors from commands arrive as plain strings. */
export function errorText(err: unknown): string {
  if (typeof err === "string")
    return err.charAt(0).toUpperCase() + err.slice(1);
  if (err instanceof Error) return err.message;
  return "Something went wrong.";
}
