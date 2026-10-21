//! Folder names on the wire. IMAP writes names that are not plain ASCII in
//! "modified UTF-7" (RFC 3501 section 5.1.3): `Entwürfe` is `Entw&APw-rfe`.
//! Names are stored as the server sends them; these convert typed names.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

/// A typed name as the server expects it.
pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut run: Vec<u16> = Vec::new();
    for c in name.chars() {
        if (' '..='~').contains(&c) {
            flush(&mut out, &mut run);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut units = [0; 2];
            run.extend_from_slice(c.encode_utf16(&mut units));
        }
    }
    flush(&mut out, &mut run);
    out
}

fn flush(out: &mut String, run: &mut Vec<u16>) {
    if run.is_empty() {
        return;
    }
    let bytes: Vec<u8> = run.iter().flat_map(|u| u.to_be_bytes()).collect();
    out.push('&');
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out.push('-');
    run.clear();
}

/// A server name as people read it. Text that is not valid modified UTF-7
/// is kept as it is.
pub fn decode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let encoded = &after[..end];
        if encoded.is_empty() {
            out.push('&');
        } else {
            match decode_run(encoded) {
                Some(text) => out.push_str(&text),
                None => out.push_str(&rest[start..start + end + 2]),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn decode_run(encoded: &str) -> Option<String> {
    let mut bits = 0u32;
    let mut count = 0;
    let mut bytes = Vec::new();
    for c in encoded.bytes() {
        let value = B64.iter().position(|b| *b == c)? as u32;
        bits = bits << 6 | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_be_bytes(*p))
        .collect();
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_stay_as_they_are() {
        assert_eq!(encode("Projects/2026"), "Projects/2026");
        assert_eq!(decode("Projects/2026"), "Projects/2026");
    }

    #[test]
    fn ampersands_and_other_letters_are_escaped() {
        assert_eq!(encode("Tom & Jerry"), "Tom &- Jerry");
        assert_eq!(encode("Entwürfe"), "Entw&APw-rfe");
        // The RFC's own example.
        assert_eq!(
            encode("~peter/mail/台北/日本語"),
            "~peter/mail/&U,BTFw-/&ZeVnLIqe-"
        );
    }

    #[test]
    fn names_round_trip() {
        for name in [
            "Entwürfe",
            "Tom & Jerry",
            "Reçus 📬",
            "日本語",
            "a&b&c",
            "Ünïcödé/Sub",
        ] {
            assert_eq!(decode(&encode(name)), name);
        }
    }

    #[test]
    fn broken_names_are_shown_as_sent() {
        assert_eq!(decode("Odd &name"), "Odd &name");
        assert_eq!(decode("Odd &!!-"), "Odd &!!-");
    }
}
