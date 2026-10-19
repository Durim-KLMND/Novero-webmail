// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Security = "implicitTls" | "startTls" | "insecure";

export interface Endpoint {
  host: string;
  port: number;
  security: Security;
}

export interface Account {
  id: number;
  displayName: string;
  email: string;
  username: string;
  imap: Endpoint;
  smtp: Endpoint;
}

export type FolderRole = "inbox" | "sent" | "drafts" | "trash" | "junk" | "archive" | "other";

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

export interface MessageSummary {
  id: number;
  uid: number;
  from: string | null;
  subject: string | null;
  date: number | null;
  flags: Flags;
  hasAttachments: boolean;
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

export interface ProviderPreset {
  name: string;
  imap: Endpoint;
  smtp: Endpoint;
  note: string | null;
}

export interface NewAccountInput {
  displayName: string;
  email: string;
  username: string;
  password: string;
  imap: Endpoint;
  smtp: Endpoint;
}

export type MailEvent =
  | { type: "syncStarted"; account: number }
  | { type: "syncProgress"; account: number; folder: string; done: number; total: number }
  | { type: "syncFinished"; account: number; error: string | null };

export const api = {
  providerPreset: (email: string) => invoke<ProviderPreset | null>("provider_preset", { email }),
  addAccount: (input: NewAccountInput) => invoke<Account>("add_account", { input }),
  listAccounts: () => invoke<Account[]>("list_accounts"),
  removeAccount: (id: number) => invoke<void>("remove_account", { id }),
  listFolders: (account: number) => invoke<Folder[]>("list_folders", { account }),
  listMessages: (folder: number, offset: number, limit: number) =>
    invoke<MessagePage>("list_messages", { folder, offset, limit }),
  openMessage: (id: number, allowRemote: boolean) =>
    invoke<ParsedMessage>("open_message", { id, allowRemote }),
  syncAccount: (account: number) => invoke<void>("sync_account", { account }),
};

export function onMailEvent(handler: (event: MailEvent) => void): Promise<UnlistenFn> {
  return listen<MailEvent>("mail-event", (e) => handler(e.payload));
}

/** Errors from commands arrive as plain strings. */
export function errorText(err: unknown): string {
  if (typeof err === "string") return err.charAt(0).toUpperCase() + err.slice(1);
  if (err instanceof Error) return err.message;
  return "Something went wrong.";
}
