//! Integration tests for changes in both directions: local changes replayed to
//! the server, server changes picked up by sync, offline replay, IDLE, and
//! drafts saved to the Drafts folder.
//!
//! Each test works in its own scratch folders so the seeded mailbox stays as it
//! is. Run with the test server up: `cargo test -p mail-sync -- --ignored`.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mail_core::{
    Account, DraftFields, Endpoint, Error, FlagKind, FolderId, FolderRole, NewAccount, Op, Security,
};
use mail_store::{NewDraft, Store};
use mail_sync::{ImapSession, push_changes, sync_folder};

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

async fn connect() -> ImapSession {
    ImapSession::connect(&endpoint(), "alice", "secret")
        .await
        .unwrap()
}

fn raw(subject: &str) -> Vec<u8> {
    format!(
        "From: Bob <bob@example.test>\r\nTo: alice@example.test\r\nSubject: {subject}\r\n\
         Message-ID: <{subject}@example.test>\r\nDate: Thu, 1 Jan 2026 10:00:00 +0000\r\n\r\n\
         Body of {subject}.\r\n"
    )
    .into_bytes()
}

/// A new server folder holding `count` unread messages, UIDs 1 to `count`.
async fn scratch_folder(label: &str, count: usize) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let name = format!("Test-{label}-{}-{nanos}", std::process::id());
    let mut session = connect().await;
    session.create_folder(&name).await.unwrap();
    for i in 1..=count {
        session
            .append(&name, None, &raw(&format!("{label}-{i}")))
            .await
            .unwrap();
    }
    session.logout().await.unwrap();
    name
}

async fn drop_folder(name: &str) {
    let mut session = connect().await;
    let _ = session.delete_folder(name).await;
    let _ = session.logout().await;
}

/// A store with Alice's account and the server's folder list.
async fn setup(dir: &std::path::Path) -> (Mutex<Store>, Account) {
    let mut store = Store::open_in_memory(dir).unwrap();
    let account = store
        .add_account(&NewAccount {
            display_name: "Alice".into(),
            email: "alice@example.test".into(),
            username: "alice".into(),
            imap: endpoint(),
            smtp: endpoint(),
        })
        .unwrap();
    let mut session = connect().await;
    let remote = session.list_folders().await.unwrap();
    session.logout().await.unwrap();
    store.sync_folders(account.id, &remote).unwrap();
    (Mutex::new(store), account)
}

async fn sync(store: &Mutex<Store>, account: &Account, folder: &str) -> FolderId {
    let id = store
        .lock()
        .unwrap()
        .folder_id(account.id, folder)
        .unwrap()
        .expect("folder is cached");
    let mut session = connect().await;
    sync_folder(store, &mut session, account.id, id, folder, &|_| {})
        .await
        .unwrap();
    session.logout().await.unwrap();
    id
}

fn uid_validity(store: &Mutex<Store>, folder: FolderId) -> Option<u32> {
    store
        .lock()
        .unwrap()
        .folder_state(folder)
        .unwrap()
        .uid_validity
}

async fn server_flags(folder: &str) -> Vec<(u32, mail_core::Flags)> {
    let mut session = connect().await;
    session.select(folder).await.unwrap();
    let mut flags = session.fetch_flags("1:*", None).await.unwrap();
    session.logout().await.unwrap();
    flags.sort_by_key(|(uid, _)| *uid);
    flags
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn flag_changes_reach_the_server_and_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let folder = scratch_folder("flags", 3).await;
    let (store, account) = setup(dir.path()).await;
    let id = sync(&store, &account, &folder).await;
    let validity = uid_validity(&store, id);

    for (uid, flag) in [(1, FlagKind::Seen), (2, FlagKind::Flagged)] {
        store
            .lock()
            .unwrap()
            .apply_op(
                account.id,
                &Op::SetFlag {
                    folder: folder.clone(),
                    uid_validity: validity,
                    uids: vec![uid],
                    flag,
                    value: true,
                },
            )
            .unwrap();
    }
    let outcome = push_changes(&store, &account, "secret", |_| {})
        .await
        .unwrap();
    assert_eq!(outcome.applied, 2);
    assert!(outcome.rejected.is_empty());
    let flags = server_flags(&folder).await;
    assert!(flags[0].1.seen && !flags[0].1.flagged);
    assert!(flags[1].1.flagged && !flags[1].1.seen);

    // Someone else reads message 3: the next sync shows it as read.
    let mut other = connect().await;
    other.select(&folder).await.unwrap();
    other.set_flag("3", FlagKind::Seen, true).await.unwrap();
    other.logout().await.unwrap();
    sync(&store, &account, &folder).await;
    let page = store.lock().unwrap().list_messages(id, 0, 10).unwrap();
    let seen: Vec<_> = page.items.iter().map(|m| (m.uid, m.flags.seen)).collect();
    assert!(seen.contains(&(3, true)), "{seen:?}");
    assert!(seen.contains(&(2, false)), "{seen:?}");

    drop_folder(&folder).await;
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn deletions_and_new_mail_on_the_server_are_picked_up() {
    let dir = tempfile::tempdir().unwrap();
    let folder = scratch_folder("server", 3).await;
    let (store, account) = setup(dir.path()).await;
    let id = sync(&store, &account, &folder).await;
    assert_eq!(store.lock().unwrap().local_uids(id).unwrap(), [1, 2, 3]);

    let mut other = connect().await;
    other.select(&folder).await.unwrap();
    other.expunge_uids("2").await.unwrap();
    other
        .append(&folder, None, &raw("server-new"))
        .await
        .unwrap();
    other.logout().await.unwrap();

    sync(&store, &account, &folder).await;
    assert_eq!(store.lock().unwrap().local_uids(id).unwrap(), [1, 3, 4]);

    drop_folder(&folder).await;
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn moves_and_deletes_reach_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let from = scratch_folder("move-from", 3).await;
    let to = scratch_folder("move-to", 0).await;
    let (store, account) = setup(dir.path()).await;
    let from_id = sync(&store, &account, &from).await;
    let to_id = sync(&store, &account, &to).await;
    let validity = uid_validity(&store, from_id);

    {
        let mut s = store.lock().unwrap();
        s.apply_op(
            account.id,
            &Op::Move {
                folder: from.clone(),
                uid_validity: validity,
                uids: vec![1],
                to: to.clone(),
            },
        )
        .unwrap();
        s.apply_op(
            account.id,
            &Op::Expunge {
                folder: from.clone(),
                uid_validity: validity,
                uids: vec![2],
            },
        )
        .unwrap();
        // Gone locally at once, before the server has heard about it.
        assert_eq!(s.local_uids(from_id).unwrap(), [3]);
    }

    let outcome = push_changes(&store, &account, "secret", |_| {})
        .await
        .unwrap();
    assert_eq!(outcome.applied, 2);
    assert_eq!(outcome.touched, std::slice::from_ref(&to));
    // The moved message shows up in the target folder.
    assert_eq!(store.lock().unwrap().local_uids(to_id).unwrap().len(), 1);
    assert_eq!(server_flags(&from).await.len(), 1);
    assert_eq!(server_flags(&to).await.len(), 1);

    // A sync of the source folder does not bring them back.
    sync(&store, &account, &from).await;
    assert_eq!(store.lock().unwrap().local_uids(from_id).unwrap(), [3]);

    drop_folder(&from).await;
    drop_folder(&to).await;
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn changes_made_offline_replay_after_reconnecting() {
    let dir = tempfile::tempdir().unwrap();
    let from = scratch_folder("offline-from", 3).await;
    let to = scratch_folder("offline-to", 0).await;
    let (store, account) = setup(dir.path()).await;
    let from_id = sync(&store, &account, &from).await;
    let validity = uid_validity(&store, from_id);

    let ops = [
        Op::SetFlag {
            folder: from.clone(),
            uid_validity: validity,
            uids: vec![1],
            flag: FlagKind::Seen,
            value: true,
        },
        Op::Move {
            folder: from.clone(),
            uid_validity: validity,
            uids: vec![2],
            to: to.clone(),
        },
        Op::Expunge {
            folder: from.clone(),
            uid_validity: validity,
            uids: vec![3],
        },
    ];
    for op in &ops {
        store.lock().unwrap().apply_op(account.id, op).unwrap();
    }

    // Offline: nothing listens on port 1, so the connection is refused.
    let mut offline = account.clone();
    offline.imap.port = 1;
    let result = push_changes(&store, &offline, "secret", |_| {}).await;
    assert!(matches!(result, Err(Error::Network(_))), "{result:?}");
    assert_eq!(store.lock().unwrap().pending_count(account.id).unwrap(), 3);
    assert_eq!(server_flags(&from).await.len(), 3, "server untouched");

    // Back online: everything replays in order.
    let outcome = push_changes(&store, &account, "secret", |_| {})
        .await
        .unwrap();
    assert_eq!(outcome.applied, 3);
    assert_eq!(store.lock().unwrap().pending_count(account.id).unwrap(), 0);
    let left = server_flags(&from).await;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0, 1);
    assert!(left[0].1.seen);
    assert_eq!(server_flags(&to).await.len(), 1);

    drop_folder(&from).await;
    drop_folder(&to).await;
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn a_refused_change_is_dropped_and_undone_locally() {
    let dir = tempfile::tempdir().unwrap();
    let folder = scratch_folder("refused", 2).await;
    let (store, account) = setup(dir.path()).await;
    let id = sync(&store, &account, &folder).await;
    let validity = uid_validity(&store, id);

    store
        .lock()
        .unwrap()
        .apply_op(
            account.id,
            &Op::Move {
                folder: folder.clone(),
                uid_validity: validity,
                uids: vec![1],
                to: "No-Such-Folder".into(),
            },
        )
        .unwrap();
    assert_eq!(store.lock().unwrap().local_uids(id).unwrap(), [2]);

    let outcome = push_changes(&store, &account, "secret", |_| {})
        .await
        .unwrap();
    assert_eq!(outcome.applied, 0);
    assert_eq!(outcome.rejected.len(), 1);
    assert_eq!(store.lock().unwrap().pending_count(account.id).unwrap(), 0);
    // The folder resynced, so the message is back where the server has it.
    assert_eq!(store.lock().unwrap().local_uids(id).unwrap(), [1, 2]);

    drop_folder(&folder).await;
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn idle_wakes_when_mail_arrives() {
    let folder = scratch_folder("idle", 0).await;

    let mut session = connect().await;
    assert!(session.has("IDLE"));
    session.select(&folder).await.unwrap();
    let (session, changed) = session.idle(Duration::from_millis(300)).await.unwrap();
    assert!(!changed, "nothing happened, so the wait times out");

    let name = folder.clone();
    let deliver = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut other = connect().await;
        other.append(&name, None, &raw("idle-1")).await.unwrap();
        other.logout().await.unwrap();
    });
    let (session, changed) = session.idle(Duration::from_secs(10)).await.unwrap();
    deliver.await.unwrap();
    assert!(changed, "new mail ends the wait");
    session.logout().await.unwrap();

    drop_folder(&folder).await;
}

/// Server copies of a draft, found by its Message-ID, with their subjects.
async fn draft_copies(folder: &str, message_id: &str) -> Vec<String> {
    let mut session = connect().await;
    session.select(folder).await.unwrap();
    let uids = session
        .search_uids(&format!("HEADER Message-ID \"<{message_id}>\" DRAFT"))
        .await
        .unwrap();
    let mut subjects = Vec::new();
    for uid in uids {
        let raw = session.fetch_raw(folder, uid).await.unwrap();
        subjects.push(
            mail_mime::parse_message(&raw, false)
                .unwrap()
                .subject
                .unwrap_or_default(),
        );
    }
    session.logout().await.unwrap();
    subjects
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn drafts_are_saved_replaced_and_removed_on_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = setup(dir.path()).await;
    // A scratch folder stands in for Drafts, so the seeded one stays as it is.
    let folder = scratch_folder("drafts", 0).await;
    let mut remote = {
        let mut session = connect().await;
        let remote = session.list_folders().await.unwrap();
        session.logout().await.unwrap();
        remote
    };
    for f in &mut remote {
        f.role = match (f.name == folder, f.role) {
            (true, _) => FolderRole::Drafts,
            (false, FolderRole::Drafts) => FolderRole::Other,
            (false, role) => role,
        };
    }
    store
        .lock()
        .unwrap()
        .sync_folders(account.id, &remote)
        .unwrap();

    let message_id = format!("{folder}@example.test");
    let draft = store
        .lock()
        .unwrap()
        .create_draft(NewDraft {
            fields: DraftFields {
                to: "bob@exa".into(),
                subject: "First".into(),
                ..DraftFields::default()
            },
            ..NewDraft::new(account.id, message_id.clone())
        })
        .unwrap();
    let outcome = push_changes(&store, &account, "secret", |_| {})
        .await
        .unwrap();
    assert_eq!(outcome.touched, std::slice::from_ref(&folder));
    assert_eq!(draft_copies(&folder, &message_id).await, ["First"]);
    let cached = sync(&store, &account, &folder).await;
    assert_eq!(store.lock().unwrap().folder_state(cached).unwrap().count, 1);

    // An edit replaces the copy instead of adding one.
    store
        .lock()
        .unwrap()
        .save_draft_fields(
            draft.id,
            &DraftFields {
                subject: "Second".into(),
                ..DraftFields::default()
            },
        )
        .unwrap();
    push_changes(&store, &account, "secret", |_| {})
        .await
        .unwrap();
    assert_eq!(draft_copies(&folder, &message_id).await, ["Second"]);

    // Nothing left to do: no connection is made.
    let idle = push_changes(&store, &account, "secret", |_| {})
        .await
        .unwrap();
    assert!(idle.touched.is_empty());

    store.lock().unwrap().discard_draft(draft.id).unwrap();
    push_changes(&store, &account, "secret", |_| {})
        .await
        .unwrap();
    assert!(draft_copies(&folder, &message_id).await.is_empty());
    assert!(
        store
            .lock()
            .unwrap()
            .drafts_to_upload(account.id)
            .unwrap()
            .is_empty()
    );

    drop_folder(&folder).await;
}
