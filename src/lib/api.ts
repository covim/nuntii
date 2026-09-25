// Typed wrappers around the Tauri commands in src-tauri/src/commands. Keep in sync with the
// serde structs there (camelCase on both sides).
import { invoke } from "@tauri-apps/api/core";

export type Security = "tls" | "starttls";
export type AccountKind = "imap" | "gmail";

export interface Account {
  id: number;
  kind: AccountKind;
  /** Account name inside the app (sidebar, "From" picker). */
  displayName: string;
  /** Name in the From header of sent mail; empty = address only. */
  senderName: string;
  sortOrder: number;
  email: string;
  imapHost: string;
  imapPort: number;
  imapSecurity: Security;
  smtpHost: string;
  smtpPort: number;
  smtpSecurity: Security;
  username: string;
  signatureId: number | null;
}

export type AccountConfig = Omit<Account, "id" | "signatureId" | "sortOrder">;

export type FolderRole = "inbox" | "sent" | "drafts" | "trash" | "archive" | "junk";

export interface Folder {
  id: number;
  accountId: number;
  remoteName: string;
  displayName: string;
  delimiter: string | null;
  role: FolderRole | null;
  selectable: boolean;
  unreadCount: number;
  totalCount: number;
}

export interface EmailAddress {
  name: string | null;
  address: string;
}

export type ListFilter = "all" | "unread" | "flagged";

export interface MessageSummary {
  id: number;
  accountId: number;
  folderId: number;
  threadId: number | null;
  subject: string;
  from: EmailAddress[];
  to: EmailAddress[];
  date: number;
  snippet: string;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  hasAttachments: boolean;
  bodyFetched: boolean;
}

export interface AttachmentInfo {
  id: number;
  idx: number;
  filename: string;
  mime: string;
  size: number;
  contentId: string | null;
  inline: boolean;
}

export interface MessageDetail extends MessageSummary {
  cc: EmailAddress[];
  text: string | null;
  htmlDocument: string | null;
  blockedRemote: number;
  remoteAllowed: boolean;
  attachments: AttachmentInfo[];
  messageIdHdr: string | null;
  references: string[];
}

export interface Signature {
  id: number;
  name: string;
  bodyText: string;
}

export interface Settings {
  gmailClientId: string;
  gmailHasSecret: boolean;
  syncWindowDays: number;
}

export interface ComposeInput {
  accountId: number;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  bodyText: string;
  bodyHtml: string | null;
  replyToMessageId: number | null;
  forwardMessageId: number | null;
  forwardAttachmentIdx: number[];
  attachmentPaths: string[];
  draftId: number | null;
}

export type ComposeMode = "new" | "reply" | "replyAll" | "forward";

/** Locally auto-saved compose draft. */
export interface LocalDraft {
  id: number | null;
  accountId: number;
  mode: ComposeMode;
  sourceMessageId: number | null;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  bodyHtml: string;
  forwardIdx: number[];
  attachmentPaths: string[];
  updatedAt: number;
}

export interface SendResult {
  sent: boolean;
  error: string | null;
}

export interface OutboxEntry {
  id: number;
  accountId: number;
  status: "pending" | "failed";
  error: string | null;
  createdAt: number;
}

// Events emitted by the backend.
export interface MailChangedEvent {
  accountId: number;
  folderIds: number[];
}
export interface AccountStatusEvent {
  accountId: number;
  state: "syncing" | "idle" | "error";
  error: string | null;
}
export interface SyncProgressEvent {
  accountId: number;
  folder: string;
  done: number;
  total: number;
}

export const api = {
  accountsList: () => invoke<Account[]>("accounts_list"),
  accountTest: (config: AccountConfig, password: string) => invoke<void>("account_test", { config, password }),
  accountAddImap: (config: AccountConfig, password: string) =>
    invoke<Account>("account_add_imap", { config, password }),
  accountAddGmail: (senderName: string | null) => invoke<Account>("account_add_gmail", { senderName }),
  accountUpdate: (id: number, displayName: string, senderName: string, signatureId: number | null) =>
    invoke<Account>("account_update", { id, displayName, senderName, signatureId }),
  accountsReorder: (ids: number[]) => invoke<void>("accounts_reorder", { ids }),
  accountUpdatePassword: (id: number, password: string) => invoke<void>("account_update_password", { id, password }),
  accountReconnect: (id: number) => invoke<void>("account_reconnect", { id }),
  accountRemove: (id: number) => invoke<void>("account_remove", { id }),
  syncNow: (accountId: number | null) => invoke<void>("sync_now", { accountId }),

  foldersList: (accountId: number | null) => invoke<Folder[]>("folders_list", { accountId }),
  messagesList: (folderId: number, offset: number, limit: number, filter: ListFilter, keepId: number | null) =>
    invoke<MessageSummary[]>("messages_list", { folderId, offset, limit, filter, keepId }),
  threadGet: (threadId: number) => invoke<MessageSummary[]>("thread_get", { threadId }),
  messageGet: (id: number, allowRemote: boolean) => invoke<MessageDetail>("message_get", { id, allowRemote }),
  senderTrust: (address: string) => invoke<void>("sender_trust", { address }),
  messagesSetFlag: (ids: number[], flag: "seen" | "flagged", value: boolean) =>
    invoke<void>("messages_set_flag", { ids, flag, value }),
  messagesMove: (ids: number[], targetFolderId: number) => invoke<void>("messages_move", { ids, targetFolderId }),
  messagesDelete: (ids: number[]) => invoke<void>("messages_delete", { ids }),
  attachmentSave: (messageId: number, idx: number, path: string) =>
    invoke<void>("attachment_save", { messageId, idx, path }),
  search: (query: string, limit?: number) => invoke<MessageSummary[]>("search", { query, limit: limit ?? null }),

  composeSend: (input: ComposeInput) => invoke<SendResult>("compose_send", { input }),
  outboxList: () => invoke<OutboxEntry[]>("outbox_list"),
  outboxDiscard: (id: number) => invoke<void>("outbox_discard", { id }),
  draftsList: () => invoke<LocalDraft[]>("drafts_list"),
  draftSave: (draft: LocalDraft) => invoke<number>("draft_save", { draft }),
  draftDelete: (id: number) => invoke<void>("draft_delete", { id }),

  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSetGmail: (clientId: string, clientSecret: string) =>
    invoke<void>("settings_set_gmail", { clientId, clientSecret }),
  settingsSetSyncWindow: (days: number) => invoke<void>("settings_set_sync_window", { days }),
  signaturesList: () => invoke<Signature[]>("signatures_list"),
  signatureSave: (id: number | null, name: string, bodyText: string) =>
    invoke<number>("signature_save", { id, name, bodyText }),
  signatureDelete: (id: number) => invoke<void>("signature_delete", { id }),
};

/** Tauri rejects with the serialized AppError string. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
