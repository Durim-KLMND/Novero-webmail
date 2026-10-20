//! MIME parsing and HTML sanitizing.
//!
//! Incoming mail is hostile input: everything shown in the UI goes through
//! [`parse_message`], which decodes the message and sanitizes its HTML.
//! [`build_message`] turns a draft into outgoing mail. [`parse_header_fields`]
//! also reads the References that conversations are grouped by.

mod compose;
mod html;
mod invite;
mod mailto;
mod parse;

pub use compose::{
    BuiltMessage, Mailbox, Prefill, ReplyMode, Sender, build_message, forward, message_id_of,
    new_message_id, parse_addresses, reopen, reply,
};
pub use html::{
    InlineImage, SanitizedHtml, clean_compose_html, inline_images, restore_inline_images,
    sanitize_html,
};
pub use invite::{InviteAnswer, build_invite_answer, calendar_text};
pub use mail_core::{Error, Result};
pub use mailto::parse_mailto;
pub use parse::{
    AttachmentInfo, ParsedMessage, bracketed, parse_header_fields, parse_message, search_text,
};
