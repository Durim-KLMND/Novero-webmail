//! HTML sanitizing for message bodies.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Sanitized HTML and what was removed from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedHtml {
    pub html: String,
    /// Remote images (and CSS with remote URLs) removed because remote content is blocked.
    pub blocked_remote: usize,
}

/// Attributes that layout-heavy email relies on.
const LAYOUT_ATTRIBUTES: &[&str] = &[
    "style",
    "align",
    "valign",
    "bgcolor",
    "width",
    "height",
    "border",
    "cellpadding",
    "cellspacing",
    "color",
    "face",
    "size",
];

/// Removes scripts, forms, event handlers, embedded objects and unsafe URLs.
///
/// `inline_images` maps Content-IDs to `data:` URIs so `cid:` images show.
/// When `allow_remote` is false, `http(s)` images and CSS containing `url(`
/// are dropped and counted, which blocks tracking pixels.
pub fn sanitize_html(
    html: &str,
    inline_images: HashMap<String, String>,
    allow_remote: bool,
) -> SanitizedHtml {
    let blocked = Arc::new(AtomicUsize::new(0));
    let counter = blocked.clone();

    let mut builder = ammonia::Builder::default();
    builder
        .add_generic_attributes(LAYOUT_ATTRIBUTES)
        .add_url_schemes(["data", "cid"])
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(move |element, attribute, value| {
            filter_attribute(
                element,
                attribute,
                value,
                &inline_images,
                allow_remote,
                &counter,
            )
        });
    let html = builder.clean(html).to_string();

    SanitizedHtml {
        html,
        blocked_remote: blocked.load(Ordering::Relaxed),
    }
}

fn filter_attribute<'u>(
    element: &str,
    attribute: &str,
    value: &'u str,
    inline_images: &HashMap<String, String>,
    allow_remote: bool,
    blocked: &AtomicUsize,
) -> Option<Cow<'u, str>> {
    let lower = value.trim().to_ascii_lowercase();
    match (element, attribute) {
        ("img", "src") => {
            if let Some(cid) = lower.strip_prefix("cid:") {
                let cid = cid.trim_matches(|c| c == '<' || c == '>');
                return inline_images.get(cid).cloned().map(Cow::Owned);
            }
            if lower.starts_with("data:image/") {
                return Some(Cow::Borrowed(value));
            }
            if lower.starts_with("http://") || lower.starts_with("https://") {
                if allow_remote {
                    return Some(Cow::Borrowed(value));
                }
                blocked.fetch_add(1, Ordering::Relaxed);
            }
            None
        }
        (_, "style") => {
            if lower.contains("expression(") {
                return None;
            }
            if lower.contains("url(") && !allow_remote {
                blocked.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            Some(Cow::Borrowed(value))
        }
        // data: and cid: URLs are only for images; a data: link could open HTML.
        (_, _) if lower.starts_with("data:") || lower.starts_with("cid:") => None,
        _ => Some(Cow::Borrowed(value)),
    }
}

/// What a formatted message being written may contain: text styles, links,
/// lists, quotes and pasted images (as `data:` URLs, sent as inline parts).
/// Anything else, such as pasted scripts, styles or remote images, is dropped
/// before the message is saved or sent.
pub fn clean_compose_html(html: &str) -> String {
    let cleaned = ammonia::Builder::empty()
        .add_tags([
            "a",
            "b",
            "strong",
            "i",
            "em",
            "u",
            "s",
            "br",
            "p",
            "div",
            "ul",
            "ol",
            "li",
            "blockquote",
            "img",
        ])
        .add_tag_attributes("a", ["href"])
        .add_tag_attributes("img", ["src", "alt", "width", "height"])
        .url_schemes(["http", "https", "mailto", "data"].into())
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("img", "src") => is_inline_image(value).then_some(value.into()),
            (_, "href") if value.trim_start().to_ascii_lowercase().starts_with("data:") => None,
            _ => Some(value.into()),
        })
        .link_rel(Some("noopener noreferrer"))
        .clean(html)
        .to_string();
    drop_sourceless_images(&cleaned)
}

/// Removes `<img>` tags whose source the filter took out (remote pictures).
fn drop_sourceless_images(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("<img") {
        let end = rest[at..].find('>').map_or(rest.len(), |e| at + e + 1);
        out.push_str(&rest[..at]);
        let tag = &rest[at..end];
        if tag.contains(" src=\"") {
            out.push_str(tag);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The image types a pasted picture may be: shown by every mail client.
const INLINE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// `data:image/...;base64,` for one of [`INLINE_TYPES`].
fn is_inline_image(src: &str) -> bool {
    inline_type(src).is_some()
}

fn inline_type(src: &str) -> Option<(&'static str, &str)> {
    let rest = src.strip_prefix("data:")?;
    let (kind, data) = rest.split_once(";base64,")?;
    let kind = INLINE_TYPES.iter().find(|t| t.eq_ignore_ascii_case(kind))?;
    Some((kind, data))
}

/// An image carried inside a message, referred to by `cid:` in its HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineImage {
    pub content_type: &'static str,
    pub content_id: String,
    pub data: Vec<u8>,
}

/// Moves the `data:` images of cleaned compose HTML into parts: their `src`
/// becomes `cid:<id>`, with ids made from `id_base` (a Message-ID without
/// brackets). The same picture twice is sent once.
pub fn inline_images(html: &str, id_base: &str) -> (String, Vec<InlineImage>) {
    use base64::Engine;
    const SRC: &str = "src=\"";
    let mut out = String::with_capacity(html.len());
    let mut images: Vec<(String, InlineImage)> = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(SRC) {
        let value_start = at + SRC.len();
        let Some(len) = rest[value_start..].find('"') else {
            break;
        };
        let value = &rest[value_start..value_start + len];
        out.push_str(&rest[..value_start]);
        let decoded = inline_type(value).and_then(|(kind, data)| {
            base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .ok()
                .map(|bytes| (kind, bytes))
        });
        match decoded {
            Some((kind, bytes)) => {
                let id = match images.iter().find(|(src, _)| src == value) {
                    Some((_, image)) => image.content_id.clone(),
                    None => {
                        let id = format!("image{}.{id_base}", images.len() + 1);
                        images.push((
                            value.to_owned(),
                            InlineImage {
                                content_type: kind,
                                content_id: id.clone(),
                                data: bytes,
                            },
                        ));
                        id
                    }
                };
                out.push_str("cid:");
                out.push_str(&id);
            }
            None => out.push_str(value),
        }
        rest = &rest[value_start + len..];
    }
    out.push_str(rest);
    (out, images.into_iter().map(|(_, image)| image).collect())
}

/// The reverse of [`inline_images`], for reopening a draft: `cid:` sources
/// found in `parts` (content id, type, bytes) become `data:` URLs again.
pub fn restore_inline_images(html: &str, parts: &[(String, String, Vec<u8>)]) -> String {
    use base64::Engine;
    let mut out = html.to_owned();
    for (id, kind, data) in parts {
        let cid = format!("src=\"cid:{id}\"");
        if out.contains(&cid) {
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            out = out.replace(&cid, &format!("src=\"data:{kind};base64,{encoded}\""));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> SanitizedHtml {
        sanitize_html(html, HashMap::new(), false)
    }

    #[test]
    fn scripts_handlers_and_forms_are_removed() {
        let out = clean(
            r#"<p onclick="steal()">Hi<script>alert(1)</script></p><form action="https://evil.test"><input name=p></form><iframe src="https://evil.test"></iframe>"#,
        );
        assert!(!out.html.contains("script"));
        assert!(!out.html.contains("onclick"));
        assert!(!out.html.contains("<form"));
        assert!(!out.html.contains("<iframe"));
        assert!(out.html.contains("<p>Hi</p>"));
    }

    #[test]
    fn javascript_links_are_removed_and_links_get_noopener() {
        let out = clean(r#"<a href="javascript:alert(1)">x</a><a href="https://ok.test">y</a>"#);
        assert!(!out.html.contains("javascript"));
        assert!(out.html.contains(r#"href="https://ok.test""#));
        assert!(out.html.contains("noopener"));
    }

    #[test]
    fn remote_images_are_blocked_and_counted() {
        let out = clean(r#"<img src="https://tracker.test/p.gif"><img src="http://a.test/b.png">"#);
        assert_eq!(out.blocked_remote, 2);
        assert!(!out.html.contains("tracker.test"));
    }

    #[test]
    fn remote_images_load_when_allowed() {
        let out = sanitize_html(r#"<img src="https://a.test/b.png">"#, HashMap::new(), true);
        assert_eq!(out.blocked_remote, 0);
        assert!(out.html.contains("https://a.test/b.png"));
    }

    #[test]
    fn css_with_remote_urls_is_blocked() {
        let out = clean(
            r#"<div style="background:url(https://t.test/x)">a</div><p style="color:red">b</p>"#,
        );
        assert_eq!(out.blocked_remote, 1);
        assert!(!out.html.contains("t.test"));
        assert!(out.html.contains("color:red"));
    }

    #[test]
    fn cid_images_become_data_uris() {
        let images =
            HashMap::from([("logo@x".to_owned(), "data:image/png;base64,AAAA".to_owned())]);
        let out = sanitize_html(
            r#"<img src="cid:logo@x"><img src="cid:missing">"#,
            images,
            false,
        );
        assert!(out.html.contains("data:image/png;base64,AAAA"));
        assert_eq!(out.html.matches("<img").count(), 2);
        assert_eq!(out.html.matches("src=").count(), 1);
    }

    #[test]
    fn data_links_are_removed() {
        let out = clean(r#"<a href="data:text/html,<script>x</script>">x</a>"#);
        assert!(!out.html.contains("data:"));
    }
}
