//! The unread state of the browser-view accounts, without Tauri. Refer to `docs/ARCHITECTURE.md` §5.
//!
//! - The Rust side reads the title of the page in an account window. [`parse_title`] keeps only the unread state.
//!   It does not keep the title.
//! - A notification of the page gives a signal with no content. [`UnreadTable::record_notice`] sets a flag.
//! - The table is in memory only. The app does not write it to the disk.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::Serialize;

/// The count does not go above this value. The user interface shows `99+` above 99.
pub const MAX_COUNT: u32 = 99_999;

/// The table ignores a second notice from the same account in this time.
pub const NOTICE_GAP: Duration = Duration::from_secs(1);

/// The prefix of the label of an account window. The account ID follows it.
pub const LABEL_PREFIX: &str = "account-";

/// The unread state of one account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Unread {
    /// The number of unread items in the title of the page. 0 if the title shows no number.
    pub count: u32,
    /// True if the title shows that unread items exist, but not their number.
    pub some: bool,
    /// True if the page sent a notification after the person last looked at the window.
    pub notice: bool,
}

/// The unread part of a page title: the count and the `some` flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TitleCount {
    pub count: u32,
    pub some: bool,
}

/// Reads the unread part of a page title.
///
/// - A `(N)` at the start gives the count N. `(99+)` and `(1,234)` are also correct.
/// - A `•` or a `●` at the start gives "unread items exist, the count is not known". Discord uses this.
/// - All other titles give no unread items.
pub fn parse_title(title: &str) -> TitleCount {
    let title = title.trim_start();
    if title.starts_with('•') || title.starts_with('●') {
        return TitleCount { count: 0, some: true };
    }
    let Some(rest) = title.strip_prefix('(') else {
        return TitleCount::default();
    };
    let Some((inside, _)) = rest.split_once(')') else {
        return TitleCount::default();
    };
    let digits = inside.strip_suffix('+').unwrap_or(inside);
    // The first and the last character must be digits. Commas can only separate the digits.
    let first_and_last_are_digits =
        digits.starts_with(|c: char| c.is_ascii_digit()) && digits.ends_with(|c: char| c.is_ascii_digit());
    if !first_and_last_are_digits || !digits.chars().all(|c| c.is_ascii_digit() || c == ',') {
        return TitleCount::default();
    }
    let count = digits
        .chars()
        .filter_map(|c| c.to_digit(10))
        .fold(0u32, |n, d| n.saturating_mul(10).saturating_add(d))
        .min(MAX_COUNT);
    TitleCount { count, some: false }
}

/// The account ID in the label of an account window, for example `account-7`. Other labels give `None`.
pub fn account_from_label(label: &str) -> Option<u64> {
    let digits = label.strip_prefix(LABEL_PREFIX)?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    state: Unread,
    /// The time of the last notice that the table accepted.
    last_notice: Option<Instant>,
}

/// The unread state of each account with an open window. The key is the account ID.
#[derive(Debug, Default)]
pub struct UnreadTable {
    entries: BTreeMap<u64, Entry>,
}

impl UnreadTable {
    /// Sets the count from a page title. Returns true if the state changed.
    pub fn set_title(&mut self, account: u64, title: &str) -> bool {
        let parsed = parse_title(title);
        let entry = self.entries.entry(account).or_default();
        let before = entry.state;
        entry.state.count = parsed.count;
        entry.state.some = parsed.some;
        entry.state != before
    }

    /// Records a notification of the page. Returns true if the state changed.
    ///
    /// The table ignores a notice that comes less than [`NOTICE_GAP`] after the last accepted notice.
    pub fn record_notice(&mut self, account: u64, now: Instant) -> bool {
        let entry = self.entries.entry(account).or_default();
        if let Some(last) = entry.last_notice
            && now.saturating_duration_since(last) < NOTICE_GAP
        {
            return false;
        }
        entry.last_notice = Some(now);
        let changed = !entry.state.notice;
        entry.state.notice = true;
        changed
    }

    /// Clears the notice flag, for example when the person looks at the window. Returns true if the state changed.
    pub fn clear_notice(&mut self, account: u64) -> bool {
        match self.entries.get_mut(&account) {
            Some(entry) if entry.state.notice => {
                entry.state.notice = false;
                true
            }
            _ => false,
        }
    }

    /// Removes the entry of an account. Returns true if there was an entry.
    pub fn remove(&mut self, account: u64) -> bool {
        self.entries.remove(&account).is_some()
    }

    /// Removes all entries. Returns true if there was an entry.
    pub fn clear(&mut self) -> bool {
        let had = !self.entries.is_empty();
        self.entries.clear();
        had
    }

    /// The state of each account in the table.
    pub fn list(&self) -> BTreeMap<u64, Unread> {
        self.entries.iter().map(|(id, entry)| (*id, entry.state)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(n: u32) -> TitleCount {
        TitleCount { count: n, some: false }
    }

    const SOME: TitleCount = TitleCount { count: 0, some: true };
    const NONE: TitleCount = TitleCount { count: 0, some: false };

    #[test]
    fn titles_of_the_sites() {
        assert_eq!(parse_title("(3) Home / X"), count(3));
        assert_eq!(parse_title("Home / X"), NONE);
        assert_eq!(parse_title("(2) Discord | #general | Server"), count(2));
        assert_eq!(parse_title("• Discord | #general | Server"), SOME);
        assert_eq!(parse_title("● Discord | #general"), SOME);
        assert_eq!(parse_title("Discord | #general | Server"), NONE);
        assert_eq!(parse_title("(12) Inbox | me@proton.me | Proton Mail"), count(12));
        assert_eq!(parse_title("Inbox | me@proton.me | Proton Mail"), NONE);
    }

    #[test]
    fn forms_of_the_count() {
        assert_eq!(parse_title("(99+) Home / X"), count(99));
        assert_eq!(parse_title("(1,234) Inbox"), count(1234));
        assert_eq!(parse_title("  (5) Home / X"), count(5));
        assert_eq!(parse_title("(0) Home / X"), count(0));
        assert_eq!(parse_title("(7)"), count(7));
        assert_eq!(parse_title(""), NONE);
    }

    #[test]
    fn titles_that_give_no_count() {
        for title in [
            "(abc) Home",
            "() Home",
            "(+) Home",
            "(,5) Home",
            "(5,) Home",
            "(3 Home",
            "(-3) Home",
            "( 3) Home",
            "(3.5) Home",
            "Home (3) / X",
            "X (3)",
            "[3] Home",
        ] {
            assert_eq!(parse_title(title), NONE, "{title}");
        }
    }

    #[test]
    fn huge_counts_stop_at_the_limit() {
        assert_eq!(parse_title("(123456) Inbox"), count(MAX_COUNT));
        assert_eq!(parse_title("(99999999999999999999999999) Inbox"), count(MAX_COUNT));
    }

    #[test]
    fn labels_of_the_windows() {
        assert_eq!(account_from_label("account-7"), Some(7));
        assert_eq!(account_from_label("account-"), None);
        assert_eq!(account_from_label("account-7x"), None);
        assert_eq!(account_from_label("account-+7"), None);
        assert_eq!(account_from_label("main"), None);
        assert_eq!(account_from_label("Account-7"), None);
    }

    #[test]
    fn table_changes_and_notices() {
        let mut table = UnreadTable::default();
        let start = Instant::now();
        assert!(!table.set_title(1, "Home / X"), "a zero count is not a change");
        assert!(table.set_title(1, "(3) Home / X"));
        assert!(!table.set_title(1, "(3) Notifications / X"));
        assert!(table.set_title(2, "• Discord"));

        assert!(table.record_notice(1, start));
        assert!(!table.record_notice(1, start + NOTICE_GAP * 2), "the flag is set");
        assert!(table.clear_notice(1));
        assert!(!table.clear_notice(1));
        assert!(
            !table.record_notice(1, start + NOTICE_GAP * 2),
            "too soon after the last notice"
        );
        assert!(table.record_notice(1, start + NOTICE_GAP * 3));

        // A new title keeps the notice flag.
        assert!(table.set_title(1, "Home / X"));
        let list = table.list();
        assert_eq!(
            list[&1],
            Unread {
                count: 0,
                some: false,
                notice: true
            }
        );
        assert_eq!(
            list[&2],
            Unread {
                count: 0,
                some: true,
                notice: false
            }
        );

        assert!(table.remove(2));
        assert!(!table.remove(2));
        assert_eq!(table.list().len(), 1);
        assert!(table.clear());
        assert!(!table.clear());
        assert!(table.list().is_empty());
    }
}
