//! Integration tests for your own folders: making, renaming and deleting them
//! on the server, with the cache following. Run with the test server up:
//! `cargo test -p mail-sync -- --ignored`.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mail_core::{Account, Endpoint, Error, NewAccount, Secret, Security};
use mail_store::Store;
use mail_sync::{ImapSession, create_folder, delete_folder, names, rename_folder, sync_folder};

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

async fn setup(dir: &std::path::Path) -> (Mutex<Store>, Account) {
    let mut store = Store::open_in_memory(dir).unwrap();
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
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    let remote = session.list_folders().await.unwrap();
    session.logout().await.unwrap();
    store.sync_folders(account.id, &remote).unwrap();
    (Mutex::new(store), account)
}

fn unique(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("{label}-{}-{nanos}", std::process::id())
}

fn names_of(store: &Mutex<Store>, account: &Account) -> Vec<String> {
    store
        .lock()
        .unwrap()
        .list_folders(account.id)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect()
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn folders_are_made_renamed_and_deleted_with_what_is_inside() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = setup(dir.path()).await;
    let top = unique("Projekte");
    let inner = format!("{top}/Entwürfe");
    let inner_wire = names::encode(&inner);

    create_folder(&store, &account, &secret(), &top)
        .await
        .unwrap();
    create_folder(&store, &account, &secret(), &inner_wire)
        .await
        .unwrap();
    assert!(names_of(&store, &account).contains(&inner_wire));

    // A message in the subfolder stays cached through the rename.
    let mut session = ImapSession::connect(&endpoint(), "alice", &secret())
        .await
        .unwrap();
    session
        .append(
            &inner_wire,
            None,
            b"From: bob@example.test\r\nSubject: Kept\r\nMessage-ID: <kept@example.test>\r\n\r\nHi.\r\n",
        )
        .await
        .unwrap();
    let id = store
        .lock()
        .unwrap()
        .folder_id(account.id, &inner_wire)
        .unwrap()
        .unwrap();
    sync_folder(&store, &mut session, account.id, id, &inner_wire, &|_| {})
        .await
        .unwrap();
    session.logout().await.unwrap();

    let renamed = unique("Archiv");
    rename_folder(&store, &account, &secret(), &top, &renamed)
        .await
        .unwrap();
    let names = names_of(&store, &account);
    assert!(names.contains(&renamed));
    assert!(!names.contains(&top));
    let moved = format!("{renamed}/{}", names::encode("Entwürfe"));
    let still = store.lock().unwrap().folder_id(account.id, &moved).unwrap();
    assert_eq!(still, Some(id), "the subfolder keeps its cache");
    assert_eq!(store.lock().unwrap().folder(id).unwrap().unwrap().total, 1);

    delete_folder(&store, &account, &secret(), &renamed)
        .await
        .unwrap();
    let names = names_of(&store, &account);
    assert!(!names.iter().any(|n| n.starts_with(&renamed)));
}

#[tokio::test]
#[ignore = "needs the test IMAP server"]
async fn the_inbox_cannot_be_renamed_or_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let (store, account) = setup(dir.path()).await;
    let err = rename_folder(&store, &account, &secret(), "INBOX", "Elsewhere")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
    let err = delete_folder(&store, &account, &secret(), "INBOX")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
}
