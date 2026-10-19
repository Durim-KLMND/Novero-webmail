// Typed wrappers around the Rust commands in src-tauri/src/commands.rs and compose.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
}

export type FolderRole =
  | "inbox"
  | "sent"
  | "drafts"
  | "trash"
  | "junk"
  | "archive"
  | "other";

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

export interface Settings {
  theme: Theme;
  conversations: boolean;
  notifications: boolean;
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
  body: string;
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
  setSignature: (account: number, signature: string) =>
    invoke<void>("set_signature", { account, signature }),
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
  saveDraft: (id: number, fields: DraftFields, upload: boolean) =>
    invoke<Draft>("save_draft", { id, fields, upload }),
  attachFiles: (id: number) => invoke<Draft>("attach_files", { id }),
  removeAttachment: (id: number, index: number) =>
    invoke<Draft>("remove_attachment", { id, index }),
  discardDraft: (id: number) => invoke<void>("discard_draft", { id }),
  sendDraft: (id: number, fields: DraftFields) =>
    invoke<void>("send_draft", { id, fields }),
  listOutbox: (account: number) =>
    invoke<OutboxItem[]>("list_outbox", { account }),
  outboxStatus: (account: number) =>
    invoke<[number, number]>("outbox_status", { account }),
  retryOutbox: (id: number) => invoke<void>("retry_outbox", { id }),
  deleteOutbox: (id: number) => invoke<void>("delete_outbox", { id }),
  editOutbox: (id: number) => invoke<Draft>("edit_outbox", { id }),
};

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
