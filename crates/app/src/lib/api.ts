// The calls to the Rust side (`src-tauri/src/main.rs`), with their types.

import { invoke } from "@tauri-apps/api/core";
import { t, type Key } from "./i18n.svelte";

export type Phase = "first_run" | "locked" | "gate_locked" | "backup" | "open";
export type Platform = "GitHub" | "Telegram" | "Email" | "X" | "Discord";
export type ProxyKind = "Http" | "Socks5";
export type Route = "Direct" | { Proxy: { kind: ProxyKind; host: string; port: number } };

export interface Settings {
  language: "en" | "zh";
  auto_lock_minutes: number;
}

export interface Status {
  phase: Phase;
  tries_left: number;
  settings: Settings;
  persona: { address: string; route: Route } | null;
}

export interface Account {
  id: number;
  persona: number;
  platform: Platform;
  display_name: string;
  handle: string;
  remote_id: string | null;
  joined: { credentials: number } | null;
  browser: { start_url: string } | null;
}

export interface TimelineItem {
  key: { account: number; source_id: string };
  author: string;
  time: number;
  /** For a known GitHub event, this is the English phrase. Show `event` instead. */
  text: string;
  /** The kind and the repository of a known GitHub event. `itemText` shows the phrase in the language of the person. */
  event: { kind: string; repo: string } | null;
  /** False if the item has no read state, for example a GitHub event. Then `read` is true. */
  has_read_state: boolean;
  read: boolean;
  /** True if the item has a web page. `openLink` opens it. The page URL stays on the Rust side. */
  has_link: boolean;
  /** True if the person can write an answer to the item. */
  can_answer: boolean;
  /** The data of a Telegram chat item. `null` for all other items. */
  chat: ChatSummary | null;
}

/** A Telegram chat item: one item for each chat. The text is the last message. */
export interface ChatSummary {
  /** The kind of the last message, if it is not text. `itemText` shows its label. */
  kind: string | null;
  /** The number of unread messages in the chat, from Telegram. */
  unread: number;
  /** True if the person marked the chat as unread in a Telegram app. */
  marked: boolean;
  /** True if the account sent the last message. */
  outgoing: boolean;
  /** True if the account can write in the chat. */
  can_write: boolean;
}

/** The state of a joined account. */
export interface SyncView {
  /** The number of unread timeline items of the account. For GitHub, only notification threads count. */
  unread: number;
  /** True if the page limit cut the list of notifications. Then the real number is larger. */
  unread_more: boolean;
  /** The time of the last sync without an error, in milliseconds. */
  last_success: number | null;
  /** The error code of the last sync, if it failed. */
  error: string | null;
  /** True if GitHub refused the token, or if Telegram ended the session. */
  needs_login: boolean;
}

/** What the dialog "Add an account" needs for Telegram. */
export interface TelegramStatus {
  /** False if PANIM has no `api_id` and `api_hash`: the build has none, and the person gave none. */
  configured: boolean;
  /** The values of the next login: PANIM's values or the own values of the person. */
  values: "builtin" | "own" | null;
  /** True if the persona has a proxy route. */
  proxy: boolean;
}

/** The setting of the Telegram app values. The `api_hash` never comes back from the Rust side. */
export interface TelegramApp {
  /** True if the build has PANIM's values. */
  builtin: boolean;
  /** True if the person selected their own values for new logins. */
  use_own: boolean;
  /** The `api_id` of the own values, if the person gave them. */
  own_api_id: number | null;
  /** The values of the next login, or `null` if PANIM has no values. */
  next_login: "builtin" | "own" | null;
}

/** How a Telegram login code comes. `other` is for a type that PANIM does not know. */
export type CodeRoute =
  | "app"
  | "sms"
  | "sms_word"
  | "sms_phrase"
  | "call"
  | "flash_call"
  | "missed_call"
  | "fragment"
  | "firebase"
  | "other";

/** A reason why PANIM cannot continue a Telegram login. */
export type TelegramUnsupported = "email_address" | "email_code" | "other_device" | "premium" | "registration";

/** The step of a Telegram login. The phone number and the code never come back from the Rust side. */
export type TelegramLogin = (
  | { state: "starting" | "wait_phone" | "ready" | "logging_out" | "closing" | "closed" }
  | {
      state: "wait_code";
      /** The route of the code. The texts `tg_code_via_<via>` describe each route. */
      via: CodeRoute;
      length: number;
      /** A short hint for some routes, for example the first letter of an SMS word. */
      detail: string;
      /** The route after a resend. `null` if Telegram offers no other route. */
      next: CodeRoute | null;
      /** The number of seconds before a resend is permitted. */
      timeout: number;
    }
  | { state: "wait_password"; hint: string }
  | { state: "unsupported"; reason: TelegramUnsupported }
) & { account: Account | null };

/** A message in the chat panel. All texts come from Telegram: show them as text only. */
export interface ChatMessage {
  id: number;
  time: number;
  outgoing: boolean;
  /** The name of the sender. Empty for an outgoing message. */
  sender: string;
  /** The text, or the caption of a media message. */
  text: string;
  /** The kind of the label for a message that is not text. */
  kind: string | null;
  sending: "pending" | "failed" | null;
}

/** The open chat panel. */
export interface ChatPanel {
  chat_id: number;
  title: string;
  can_write: boolean;
  /** The oldest message first. */
  messages: ChatMessage[];
}

/** The result of the removal of an account. */
export interface Removed {
  /** True if Telegram did not confirm the end of the session. */
  session_left: boolean;
}

/** A device login of GitHub that waits for the person. */
export interface LoginView {
  user_code: string;
  verification_uri: string;
  /** The number of seconds between two polls. */
  interval: number;
  expires_in: number;
}

export type LoginPoll = { pending: { interval: number } } | { done: Account };

/** The unread state of a browser-view account with an open window. */
export interface Unread {
  /** The number in the title of the page. 0 if the title shows no number. */
  count: number;
  /** True if unread items exist, but the title does not show their number. */
  some: boolean;
  /** True if the page sent a notification after the person last looked at the window. */
  notice: boolean;
}

/** The Rust side sends this event to the main window when the unread state changes. */
export const UNREAD_CHANGED = "unread-changed";

/** The Rust side sends this event when the timeline or the state of a joined account changes. */
export const TIMELINE_CHANGED = "timeline-changed";

/** The Rust side sends this event when the messages of the open chat panel change. */
export const TELEGRAM_CHAT_CHANGED = "telegram-chat-changed";

/** The proof state of one account. */
export interface ProofView {
  account: number;
  /** True if the platform has a numeric id, but PANIM does not know the id of the account. */
  id_needed: boolean;
  /** True if the person can give the handle and the id. This is true for an account without a connector. */
  identity_editable: boolean;
  /** The kept block: six lines. */
  block: string | null;
  /** The date of the kept block, YYYY-MM-DD. */
  date: string | null;
  /** The URL where the person posted the block. */
  location: string | null;
}

export type Verdict = "correct" | "bad_signature" | "other_account" | "not_block";

/** One block in a gist. All texts come from the gist: show them as text only. */
export interface BlockView {
  file: string;
  verdict: Verdict;
  persona: string | null;
  domain: "proof" | "revoke" | null;
  date: string | null;
  id: string | null;
  handle: string | null;
  platform: string | null;
  /** For a correct block: true if it is the current state of its persona and account. */
  current: boolean | null;
}

export interface GistCheck {
  owner: { id: number; login: string } | null;
  result: "blocks" | "no_block" | "secret";
  blocks: BlockView[];
  /** True if the gist has more blocks than the check gives. */
  cut: boolean;
}

export interface OwnCheck {
  proof: ProofView;
  check: GistCheck;
}

/** The result of the check of a pasted block. PANIM checked the form and the signature only. */
export interface PastedView {
  persona: string;
  platform: string;
  id: string | null;
  handle: string | null;
  domain: "proof" | "revoke";
  date: string;
  /** True if the block names the persona of the person. */
  own: boolean;
}

/** A pasted text has no more than this number of bytes. The Rust side has the same limit. */
export const MAX_PASTE = 16 * 1024;

export interface AppError {
  code: string;
  tries_left: number | null;
  detail: string;
}

export const MIN_PASSCODE = 8;

/** The text for an error from the Rust side. */
export function errorText(error: unknown): string {
  const e = error as Partial<AppError>;
  switch (e?.code) {
    case "wrong_passcode":
      return t("tries_left", { n: e.tries_left ?? 0 });
    case "passcode_too_short":
      return t("err_passcode_too_short", { n: MIN_PASSCODE });
    case "damaged":
      return t("err_damaged", { detail: e.detail ?? "" });
    case "gate_locked":
    case "wrong_recovery_key":
    case "bad_key_file":
    case "wrong_key_file_password":
    case "in_use":
    case "browser_login_in_use":
    case "file_exists":
    case "bad_proxy":
    case "locked":
    case "bad_words":
    case "words_differ":
    case "account_exists":
    case "github_login_expired":
    case "github_login_denied":
    case "github_login_none":
    case "network":
    case "github_bad_token":
    case "github_rate_limit":
    case "github_no_access":
    case "github_not_found":
    case "answer_empty":
    case "answer_no_target":
    case "no_link":
    case "no_window":
    case "github_no_unread":
    case "bad_gist":
    case "proof_no_block":
    case "proof_short_block":
    case "proof_bad_byte":
    case "proof_bad_id":
    case "proof_bad_handle":
    case "proof_bad_date":
    case "proof_no_account":
    case "proof_bad_signature":
    case "proof_id_missing":
    case "proof_key":
    case "proof_none":
    case "proof_not_github":
    case "proof_identity_fixed":
    case "proof_bad_location":
    case "own_secret":
    case "own_no_block":
    case "own_other_account":
    case "own_bad_signature":
    case "own_other_persona":
    case "own_not_block":
    case "own_revoked":
    case "own_cut":
    case "telegram_not_configured":
    case "telegram_proxy_name":
    case "telegram_bad_input":
    case "telegram_no_client":
    case "telegram_timeout":
    case "telegram_flood_unknown":
    case "telegram_code_invalid":
    case "telegram_code_expired":
    case "telegram_password_invalid":
    case "telegram_phone_invalid":
    case "telegram_banned":
    case "telegram_app_refused":
    case "telegram_database":
    case "telegram_logged_out":
    case "telegram_no_write":
    case "telegram_no_chat":
    case "telegram_login_none":
    case "telegram_no_unread":
    case "telegram_offline":
    case "telegram_resend_none":
    case "telegram_resend_state":
    case "telegram_values_missing":
    case "telegram_values_invalid":
    case "telegram_values_none":
    case "bad_backup":
    case "exists":
      return t(`err_${e.code}` as Key);
    case "proof_bad_line":
    case "proof_unknown_platform":
    case "paste_too_long":
    case "github_scope_missing":
    case "github_login_failed":
    case "github_error":
    case "telegram_flood":
    case "telegram_too_long":
    case "telegram_error":
    case "telegram_resend_early":
      return t(`err_${e.code}` as Key, { detail: e.detail ?? "" });
    default:
      return t("err_other", { detail: e?.detail ?? String(error) });
  }
}

export const api = {
  status: () => invoke<Status>("status"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  create: (passcode: string) => invoke<void>("create", { passcode }),
  unlock: (passcode: string) => invoke<void>("unlock", { passcode }),
  recover: (path: string, password: string, passcode: string) => invoke<void>("recover", { path, password, passcode }),
  recoverWithWords: (words: string, passcode: string) => invoke<void>("recover_with_words", { words, passcode }),
  recoveryWords: (passcode: string | null) => invoke<string[]>("recovery_words", { passcode }),
  checkWords: (answers: [number, string][]) => invoke<void>("check_words", { answers }),
  exportKeyFile: (password: string, path: string) => invoke<void>("export_key_file", { password, path }),
  changePasscode: (current: string, passcode: string) => invoke<void>("change_passcode", { current, passcode }),
  lock: () => invoke<void>("lock"),
  setRoute: (route: Route) => invoke<void>("set_route", { route }),
  accounts: () => invoke<Account[]>("accounts"),
  addBrowserAccount: (platform: Platform, displayName: string, handle: string) =>
    invoke<Account>("add_browser_account", { platform, displayName, handle }),
  removeAccount: (id: number) => invoke<Removed>("remove_account", { id }),
  timeline: (account: number | null, unreadOnly: boolean) =>
    invoke<TimelineItem[]>("timeline", { account, unreadOnly }),
  openAccount: (id: number) => invoke<void>("open_account", { id }),
  /** The key is the account ID. Only accounts with an open window have an entry. */
  unread: () => invoke<Record<string, Unread>>("unread"),
  githubLoginStart: () => invoke<LoginView>("github_login_start"),
  githubLoginPoll: () => invoke<LoginPoll>("github_login_poll"),
  githubLoginCancel: () => invoke<void>("github_login_cancel"),
  /** Opens the device page of GitHub in the browser profile of the persona. */
  openGithubLogin: () => invoke<void>("open_github_login"),
  /** Opens the page of a timeline item in the browser profile of the persona. */
  openLink: (account: number, sourceId: string) => invoke<void>("open_link", { account, sourceId }),
  syncNow: () => invoke<void>("sync_now"),
  /** The key is the account ID. Only joined accounts have an entry. */
  syncState: () => invoke<Record<string, SyncView>>("sync_state"),
  setRead: (account: number, sourceId: string, read: boolean) =>
    invoke<void>("set_read", { account, sourceId, read }),
  /** The result is the web page of the new answer. For Telegram, the result is empty. */
  answer: (account: number, sourceId: string, text: string) => invoke<string>("answer", { account, sourceId, text }),
  proofs: () => invoke<ProofView[]>("proofs"),
  /** With `replace`, the new block replaces a different kept block. Without it, that case gives `proof_replace`. */
  makeProof: (account: number, replace: boolean) => invoke<ProofView>("make_proof", { account, replace }),
  setIdentity: (account: number, handle: string, id: string) =>
    invoke<Account>("set_identity", { account, handle, id }),
  setProofLocation: (account: number, location: string) =>
    invoke<ProofView>("set_proof_location", { account, location }),
  checkOwnProof: (account: number, gist: string) => invoke<OwnCheck>("check_own_proof", { account, gist }),
  checkGithubProof: (gist: string) => invoke<GistCheck>("check_github_proof", { gist }),
  checkPastedProof: (text: string) => invoke<PastedView>("check_pasted_proof", { text }),
  /** Opens gist.github.com in the browser profile of the persona. */
  openGistPage: () => invoke<void>("open_gist_page"),
  telegramStatus: () => invoke<TelegramStatus>("telegram_status"),
  telegramLoginStart: () => invoke<TelegramLogin>("telegram_login_start"),
  telegramLoginPhone: (phone: string) => invoke<TelegramLogin>("telegram_login_phone", { phone }),
  telegramLoginCode: (code: string) => invoke<TelegramLogin>("telegram_login_code", { code }),
  telegramLoginPassword: (password: string) => invoke<TelegramLogin>("telegram_login_password", { password }),
  /** Sends the login code again, on the next route. The result has the new route and the new timeout. */
  telegramLoginResend: () => invoke<TelegramLogin>("telegram_login_resend"),
  telegramLoginCancel: () => invoke<void>("telegram_login_cancel"),
  /** Opens the chat panel for the chat of a timeline item, with its last messages. */
  telegramChatOpen: (account: number, sourceId: string) =>
    invoke<ChatPanel>("telegram_chat_open", { account, sourceId }),
  /** The messages of the open chat panel. Call it after the event `TELEGRAM_CHAT_CHANGED`. */
  telegramChat: (account: number, sourceId: string) => invoke<ChatPanel>("telegram_chat", { account, sourceId }),
  telegramChatClose: (account: number) => invoke<void>("telegram_chat_close", { account }),
  telegramApp: () => invoke<TelegramApp>("telegram_app"),
  /** With `apiId` and `apiHash`, the Rust side keeps them as the own values. The change is only for new logins. */
  setTelegramApp: (useOwn: boolean, apiId: string | null, apiHash: string | null) =>
    invoke<TelegramApp>("set_telegram_app", { useOwn, apiId, apiHash }),
  forgetTelegramApp: () => invoke<TelegramApp>("forget_telegram_app"),
  /** Writes a backup of the vault to `path`. The result is the number of bytes. */
  backupVault: (path: string) => invoke<number>("backup_vault", { path }),
  /** Restores a vault from a backup. Only the first start can do this. */
  restoreVault: (path: string) => invoke<void>("restore_vault", { path }),
};
