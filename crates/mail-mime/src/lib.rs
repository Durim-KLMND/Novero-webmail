//! MIME parsing and HTML sanitizing.
//!
//! Incoming mail is hostile input: everything shown in the UI goes through
//! [`parse_message`], which decodes the message and sanitizes its HTML.
//! [`build_message`] turns a draft into outgoing mail. [`parse_header_fields`]
//! also reads the References that conversations are grouped by.

mod compose;
mod html;
mod mailto;
mod parse;

pub use compose::{
    BuiltMessage, Mailbox, Prefill, ReplyMode, Sender, build_message, forward, message_id_of,
    new_message_id, parse_addresses, reopen, reply,
};
pub use html::{SanitizedHtml, clean_compose_html, sanitize_html};
pub use mail_core::{Error, Result};
pub use mailto::parse_mailto;
pub use parse::{
    AttachmentInfo, ParsedMessage, bracketed, parse_header_fields, parse_message, search_text,
};
