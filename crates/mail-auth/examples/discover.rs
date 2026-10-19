//! Prints the settings found for each address given: `cargo run -p mail-auth --example discover -- me@example.org`
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let discoverer = mail_auth::discover::Discoverer::new();
    for email in std::env::args().skip(1) {
        println!("{email}: {:#?}", discoverer.discover(&email).await);
    }
}
