//! MIME parsing and HTML sanitizing.
//!
//! Incoming mail is hostile input: everything shown in the UI goes through
//! [`parse_message`], which decodes the message and sanitizes its HTML.
//! Building outgoing mail (phase 3) and threading (phase 5) come later.

mod html;
mod parse;

pub use html::{SanitizedHtml, sanitize_html};
pub use mail_core::{Error, Result};
pub use parse::{AttachmentInfo, ParsedMessage, parse_header_fields, parse_message};
