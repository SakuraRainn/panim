//! The app ACL manifest. Refer to `docs/ARCHITECTURE.md` §2a.
//!
//! With this manifest, Tauri examines each app command against the capabilities. Each command in the list gets the
//! permissions `allow-<command>` and `deny-<command>`, with `-` in place of `_`. Add each new command to this list.

const COMMANDS: &[&str] = &[
    "status",
    "set_settings",
    "create",
    "unlock",
    "recover",
    "recover_with_words",
    "recovery_words",
    "check_words",
    "export_key_file",
    "change_passcode",
    "lock",
    "set_route",
    "accounts",
    "add_browser_account",
    "remove_account",
    "timeline",
    "open_account",
    "unread",
    "page_notice",
    "github_login_start",
    "github_login_poll",
    "github_login_cancel",
    "open_github_login",
    "open_link",
    "sync_now",
    "sync_state",
    "set_read",
    "answer",
    "check_github_proof",
    "proofs",
    "make_proof",
    "set_identity",
    "set_proof_location",
    "check_own_proof",
    "check_pasted_proof",
    "open_gist_page",
    "telegram_status",
    "telegram_login_start",
    "telegram_login_phone",
    "telegram_login_code",
    "telegram_login_password",
    "telegram_login_resend",
    "telegram_login_cancel",
    "telegram_chat_open",
    "telegram_chat",
    "telegram_chat_close",
    "telegram_app",
    "set_telegram_app",
    "forget_telegram_app",
    "backup_vault",
    "restore_vault",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("the Tauri build step failed");
}
