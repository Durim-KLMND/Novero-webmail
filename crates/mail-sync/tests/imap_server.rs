//! Integration tests against the Dovecot test server in `testing/`.
//!
//! Start it with `docker compose -f testing/docker-compose.yml up -d --build --wait`,
//! then run `cargo test -p mail-sync -- --ignored`. Override the address with
//! `MAIL_TEST_IMAP_HOST` and `MAIL_TEST_IMAP_PORT`.

use std::sync::Mutex;

use mail_core::{Endpoint, Event, FolderRole, NewAccount, Secret, Security};
use mail_store::Store;
use mail_sync::{ImapSession, Scope, fetch_message, sync_account};

/// Alice's password on the test servers.
fn secret() -> Secret {
    Secret::password("secret")
}

fn endpoint() -> Endpoint {
    Endpoint {
        host: std::env::var("MAIL_TEST_IMAP_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MAIL_TEST_IMAP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(1143),
        security: Security::Insecure,
    }
}

fn store_with_alice(dir: &std::path::Path) -> (Mutex<Store>, mail_core::Account) {
    let store = Store::open_in_memory(dir).unwrap();
    let account = store
        .add_account(&NewAccount {
            auth: Default::default(),
            display_name: "Alice".into(),
            email: "alice@example.test".into(),
            username: "alice".into(),
            imap: endpoint(),
            smtp: endpoint(),
        })
        .unwrap();
    (Mutex::new(store), account)
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn lists_folders_with_special_use_roles() {
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let folders = session.list_folders().await.unwrap();
    let role = |name: &str| folders.iter().find(|f| f.name == name).map(|f| f.role);
    assert_eq!(role("INBOX"), Some(FolderRole::Inbox));
    assert_eq!(role("Sent"), Some(FolderRole::Sent));
    assert_eq!(role("Drafts"), Some(FolderRole::Drafts));
    assert_eq!(role("Trash"), Some(FolderRole::Trash));
    assert_eq!(role("Archive"), Some(FolderRole::Archive));
    session.logout().await.unwrap();
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn fetches_the_newest_hundred_headers() {
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let headers = session.fetch_latest_headers("INBOX", 100).await.unwrap();

    assert_eq!(headers.len(), 100);
    assert!(
        headers.windows(2).all(|w| w[0].uid > w[1].uid),
        "newest first"
    );
    let newest = &headers[0];
    assert_eq!(newest.subject.as_deref(), Some("Test message 150"));
    assert_eq!(
        newest.message_id.as_deref(),
        Some("<seed-150@example.test>")
    );
    assert_eq!(
        newest.from.as_deref(),
        Some("Sender 3 <sender3@example.test>")
    );
    assert!(newest.date.is_some());
    assert!(newest.flags.seen, "message 150 is seeded as read");
    assert!(!headers[1].flags.seen, "message 149 is seeded as unread");
    session.logout().await.unwrap();
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn reports_folder_state_on_select() {
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let status = session.select("INBOX").await.unwrap();
    assert_eq!(status.exists, 150);
    assert!(status.uid_validity.is_some());
    assert_eq!(status.uid_next, Some(151));
    session.logout().await.unwrap();
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn wrong_password_is_an_auth_error() {
    let result = ImapSession::connect(&endpoint(), "alice", &Secret::password("wrong")).await;
    match result {
        Err(mail_core::Error::Auth(reason)) => {
            // The server's own words, without the response code.
            assert_eq!(reason, "the server said “Authentication failed.”");
        }
        Err(other) => panic!("expected a login error, got {other}"),
        Ok(_) => panic!("a wrong password was accepted"),
    }
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn full_sync_fills_every_folder_and_a_second_sync_adds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = store_with_alice(dir.path());

    let events = Mutex::new(Vec::new());
    sync_account(&store, &account, &secret(), Scope::All, |e| {
        events.lock().unwrap().push(e)
    })
    .await
    .unwrap();

    let folders = store.lock().unwrap().list_folders(account.id).unwrap();
    let count = |name: &str| folders.iter().find(|f| f.name == name).unwrap().total;
    assert_eq!(folders[0].name, "INBOX", "inbox sorts first");
    assert_eq!(count("INBOX"), 150);
    assert_eq!(count("Sent"), 5);
    assert_eq!(folders[0].unread, 100);

    let events = events.into_inner().unwrap();
    assert!(events.contains(&Event::SyncProgress {
        account: account.id,
        folder: "INBOX".into(),
        done: 150,
        total: 150,
    }));

    let again = Mutex::new(Vec::new());
    sync_account(&store, &account, &secret(), Scope::All, |e| {
        again.lock().unwrap().push(e)
    })
    .await
    .unwrap();
    let fetched: u32 = again
        .into_inner()
        .unwrap()
        .iter()
        .map(|e| match e {
            Event::SyncProgress { total, .. } => *total,
            _ => 0,
        })
        .sum();
    assert_eq!(fetched, 0, "nothing new to fetch");
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn interrupted_sync_is_completed_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = store_with_alice(dir.path());

    // Pretend a first sync stopped after the newest 20 inbox messages.
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let remote = session.list_folders().await.unwrap();
    let newest = session.fetch_latest_headers("INBOX", 20).await.unwrap();
    session.logout().await.unwrap();
    let inbox = {
        let mut s = store.lock().unwrap();
        let ids = s.sync_folders(account.id, &remote).unwrap();
        let inbox = ids.iter().find(|(_, f)| f.name == "INBOX").unwrap().0;
        s.insert_headers(inbox, &newest).unwrap();
        inbox
    };

    sync_account(&store, &account, &secret(), Scope::All, |_| {})
        .await
        .unwrap();
    assert_eq!(
        store.lock().unwrap().folder_state(inbox).unwrap().count,
        150
    );
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn opening_a_message_fetches_and_caches_its_body() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = store_with_alice(dir.path());
    sync_account(&store, &account, &secret(), Scope::All, |_| {})
        .await
        .unwrap();

    let inbox = store.lock().unwrap().list_folders(account.id).unwrap()[0].id;
    let id = store
        .lock()
        .unwrap()
        .list_messages(inbox, 0, 1)
        .unwrap()
        .items[0]
        .id;
    let raw = fetch_message(&store, &account, &secret(), id)
        .await
        .unwrap();
    let parsed = mail_mime::parse_message(&raw, false).unwrap();
    assert_eq!(parsed.subject.as_deref(), Some("Test message 150"));
    assert_eq!(
        parsed.text.as_deref().map(str::trim),
        Some("Body of test message 150.")
    );

    // The second open reads from disk, even with a wrong password.
    let again = fetch_message(&store, &account, &Secret::password("wrong"), id)
        .await
        .unwrap();
    assert_eq!(again, raw);
}

/// An RFC 5322 date for a Unix time, e.g. "Mon, 19 Oct 2026 09:00:00 +0000".
fn rfc_date(secs: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{}, {day:02} {} {year} {:02}:{:02}:{:02} +0000",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[month as usize - 1],
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[test]
fn rfc_dates_are_formatted() {
    assert_eq!(rfc_date(1_792_400_400), "Mon, 19 Oct 2026 09:00:00 +0000");
}

/// A mailbox of its own: the test server accepts any user name with the
/// password "secret" and starts it empty.
fn fresh_user(dir: &std::path::Path, label: &str) -> (Mutex<Store>, mail_core::Account) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let user = format!("{label}{nanos}");
    let store = Store::open_in_memory(dir).unwrap();
    let account = store
        .add_account(&NewAccount {
            auth: Default::default(),
            display_name: user.clone(),
            email: format!("{user}@example.test"),
            username: user,
            imap: endpoint(),
            smtp: endpoint(),
        })
        .unwrap();
    (Mutex::new(store), account)
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn new_mail_is_announced_threaded_and_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = fresh_user(dir.path(), "newmail");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let message = |id: &str, reply_to: Option<&str>, subject: &str, body: &str, age: i64| {
        let mut raw = format!(
            "From: Carol <carol@example.test>\r\nTo: {}\r\nSubject: {subject}\r\n\
             Date: {}\r\nMessage-ID: <{id}@example.test>\r\n",
            account.email,
            rfc_date(now - age)
        );
        if let Some(parent) = reply_to {
            raw.push_str(&format!("In-Reply-To: <{parent}@example.test>\r\n"));
        }
        raw.push_str(&format!("Content-Type: text/plain\r\n\r\n{body}\r\n"));
        raw.into_bytes()
    };
    let mut session = ImapSession::connect(&account.imap, &account.username, &secret())
        .await
        .unwrap();
    session
        .append(
            "INBOX",
            None,
            &message(
                "first",
                None,
                "Tasting",
                "The pistachio tasting is on Friday.",
                600,
            ),
        )
        .await
        .unwrap();
    // Older than the prefetch window: listed, but its body waits until opened.
    session
        .append(
            "INBOX",
            Some("(\\Seen)"),
            &message(
                "old",
                None,
                "Old news",
                "Archived walnut notes.",
                90 * 86_400,
            ),
        )
        .await
        .unwrap();

    let first = sync_account(&store, &account, &secret(), Scope::All, |_| {})
        .await
        .unwrap();
    assert!(first.new_mail.is_empty(), "a first sync announces nothing");
    let search = |q: &str| {
        store
            .lock()
            .unwrap()
            .search(&mail_store::SearchQuery::parse(q), 0, 10)
            .unwrap()
            .total
    };
    assert_eq!(
        search("pistachio"),
        1,
        "recent bodies are downloaded and indexed"
    );
    assert_eq!(search("walnut"), 0, "old bodies are not");
    assert_eq!(search("old news"), 1, "but their headers are");

    session
        .append(
            "INBOX",
            None,
            &message("reply", Some("first"), "Re: Tasting", "Count me in.", 60),
        )
        .await
        .unwrap();
    session
        .append(
            "INBOX",
            Some("(\\Seen)"),
            &message("seen", None, "Already read", "Read elsewhere.", 30),
        )
        .await
        .unwrap();
    let second = sync_account(&store, &account, &secret(), Scope::Folder("INBOX"), |_| {})
        .await
        .unwrap();
    let announced: Vec<_> = second
        .new_mail
        .iter()
        .map(|m| m.subject.clone().unwrap())
        .collect();
    assert_eq!(announced, ["Re: Tasting"], "only new unread mail");

    let inbox = store
        .lock()
        .unwrap()
        .folder_id(account.id, "INBOX")
        .unwrap()
        .unwrap();
    let page = store
        .lock()
        .unwrap()
        .list(&mail_store::Mailbox::Folder { id: inbox }, true, 0, 10)
        .unwrap();
    assert_eq!(page.total, 3, "the reply joins its conversation");
    let tasting = page
        .items
        .iter()
        .find(|m| m.subject.as_deref() == Some("Re: Tasting"))
        .unwrap();
    assert_eq!(tasting.ids.len(), 2);
    assert!(!tasting.flags.seen);
    session.logout().await.unwrap();
}
