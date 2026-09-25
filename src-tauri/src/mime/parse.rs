//! Parsing of headers (sync) and complete messages (reading pane, indexing, forwarding).

use mail_parser::{Address, HeaderValue, MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddress {
    pub name: Option<String>,
    pub address: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedHeader {
    pub subject: String,
    pub from: Vec<EmailAddress>,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub date: Option<i64>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub has_attachments_hint: bool,
}

#[derive(Debug, Clone)]
pub struct ParsedAttachment {
    pub idx: usize,
    pub filename: String,
    pub mime: String,
    pub size: usize,
    pub content_id: Option<String>,
    pub inline: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedMessage {
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<ParsedAttachment>,
}

fn addresses(addr: Option<&Address<'_>>) -> Vec<EmailAddress> {
    let Some(addr) = addr else { return Vec::new() };
    addr.iter()
        .filter_map(|a| {
            Some(EmailAddress {
                name: a.name().map(str::to_string).filter(|n| !n.trim().is_empty()),
                address: a.address()?.to_string(),
            })
        })
        .collect()
}

fn id_list(v: &HeaderValue<'_>) -> Vec<String> {
    match v {
        HeaderValue::Text(t) => vec![t.to_string()],
        HeaderValue::TextList(l) => l.iter().map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}

fn header_of(msg: &mail_parser::Message<'_>) -> ParsedHeader {
    let content_type = msg
        .root_part()
        .content_type()
        .map(|c| {
            format!(
                "{}/{}",
                c.c_type.to_ascii_lowercase(),
                c.c_subtype.as_deref().unwrap_or("").to_ascii_lowercase()
            )
        })
        .unwrap_or_default();
    ParsedHeader {
        subject: msg.subject().unwrap_or("").trim().to_string(),
        from: addresses(msg.from()),
        to: addresses(msg.to()),
        cc: addresses(msg.cc()),
        date: msg.date().map(|d| d.to_timestamp()),
        message_id: msg.message_id().map(str::to_string),
        in_reply_to: id_list(msg.in_reply_to()).into_iter().next(),
        references: id_list(msg.references()),
        has_attachments_hint: content_type == "multipart/mixed",
    }
}

/// Parses a bare header block as delivered by `BODY.PEEK[HEADER]`.
pub fn parse_header(raw_header: &[u8]) -> ParsedHeader {
    MessageParser::default()
        .parse_headers(raw_header)
        .map(|m| header_of(&m))
        .unwrap_or_default()
}

pub fn parse_message(raw: &[u8]) -> ParsedMessage {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return ParsedMessage {
            text: Some(String::from_utf8_lossy(raw).into_owned()),
            ..Default::default()
        };
    };

    let text = msg.body_text(0).map(|t| t.into_owned());
    let html = msg
        .html_bodies()
        .next()
        .filter(|p| p.is_text_html())
        .and_then(|_| msg.body_html(0))
        .map(|h| h.into_owned());

    let attachments = msg
        .attachments()
        .enumerate()
        .map(|(idx, part)| {
            let mime = part
                .content_type()
                .map(|c| match &c.c_subtype {
                    Some(sub) => format!("{}/{}", c.c_type, sub),
                    None => c.c_type.to_string(),
                })
                .unwrap_or_else(|| "application/octet-stream".into())
                .to_ascii_lowercase();
            let inline = part
                .content_disposition()
                .map(|d| d.c_type.eq_ignore_ascii_case("inline"))
                .unwrap_or(false);
            let filename = part
                .attachment_name()
                .map(str::to_string)
                .or_else(|| part.message().and_then(|m| m.subject()).map(|s| format!("{s}.eml")))
                .unwrap_or_else(|| format!("anhang-{}", idx + 1));
            ParsedAttachment {
                idx,
                filename,
                mime,
                size: part.contents().len(),
                content_id: part.content_id().map(|c| c.trim_matches(['<', '>']).to_string()),
                inline,
            }
        })
        .collect();

    ParsedMessage {
        text,
        html,
        attachments,
    }
}

/// Decoded content of the n-th attachment.
pub fn attachment_bytes(raw: &[u8], idx: usize) -> Option<Vec<u8>> {
    let msg = MessageParser::default().parse(raw)?;
    let part = msg.attachments().nth(idx)?;
    // Attached messages (message/rfc822) are exported verbatim.
    Some(match part.message() {
        Some(inner) => inner.raw_message().to_vec(),
        None => part.contents().to_vec(),
    })
}

/// Short single-line preview for the message list.
pub fn snippet(text: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(max_chars);
    let mut last_space = false;
    for line in text.lines() {
        let trimmed = line.trim();
        // Skip quoted replies so the preview shows the new content.
        if trimmed.starts_with('>') {
            continue;
        }
        for ch in trimmed.chars().chain(std::iter::once(' ')) {
            let ws = ch.is_whitespace();
            if ws && (last_space || out.is_empty()) {
                continue;
            }
            out.push(if ws { ' ' } else { ch });
            last_space = ws;
            if out.chars().count() >= max_chars {
                return out.trim_end().to_string();
            }
        }
    }
    out.trim_end().to_string()
}

/// Very small HTML → text conversion for previews and search when a mail has no text part.
pub fn html_to_text(html: &str) -> String {
    let cleaned = ammonia::Builder::empty().clean(html).to_string();
    cleaned
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Subject without reply/forward prefixes, for threading and "Re:" composition.
pub fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let stripped = ["re:", "aw:", "fw:", "fwd:", "wg:", "antw:"]
            .iter()
            .find(|p| lower.starts_with(**p))
            .map(|p| s[p.len()..].trim_start());
        match stripped {
            Some(rest) => s = rest,
            None => return s.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn parses_encoded_headers() {
        let h = parse_header(
            b"From: =?UTF-8?Q?J=C3=BCrgen_M=C3=BCller?= <jm@example.de>\r\n\
              To: a@example.com, \"B, C\" <bc@example.com>\r\n\
              Subject: =?ISO-8859-1?Q?Gr=FC=DFe?=\r\n\
              Date: Tue, 1 Sep 2026 10:00:00 +0200\r\n\
              Message-ID: <abc@example.de>\r\n\
              References: <r1@x> <r2@x>\r\n\
              In-Reply-To: <r2@x>\r\n\r\n",
        );
        assert_eq!(h.subject, "Grüße");
        assert_eq!(h.from[0].name.as_deref(), Some("Jürgen Müller"));
        assert_eq!(h.to.len(), 2);
        assert_eq!(h.to[1].address, "bc@example.com");
        assert_eq!(h.message_id.as_deref(), Some("abc@example.de"));
        assert_eq!(h.references, vec!["r1@x", "r2@x"]);
        assert_eq!(h.in_reply_to.as_deref(), Some("r2@x"));
        assert!(h.date.is_some());
    }

    #[test]
    fn parses_multipart_with_attachment_and_inline_image() {
        let m = parse_message(&fixture("multipart.eml"));
        assert_eq!(parse_header(&fixture("multipart.eml")).subject, "Angebot");
        assert!(m.text.as_deref().unwrap().contains("anbei das Angebot"));
        assert!(m.html.as_deref().unwrap().contains("<b>Angebot</b>"));
        assert_eq!(m.attachments.len(), 2);
        let pdf = m.attachments.iter().find(|a| a.mime == "application/pdf").unwrap();
        assert_eq!(pdf.filename, "angebot.pdf");
        assert!(!pdf.inline);
        let img = m.attachments.iter().find(|a| a.content_id.is_some()).unwrap();
        assert_eq!(img.content_id.as_deref(), Some("logo@nuntii"));
        assert!(img.inline);
        assert_eq!(attachment_bytes(&fixture("multipart.eml"), pdf.idx).unwrap(), b"%PDF-1.4 test");
    }

    #[test]
    fn decodes_latin1_body() {
        let m = parse_message(&fixture("latin1.eml"));
        assert!(m.text.unwrap().contains("Größe"));
    }

    #[test]
    fn survives_broken_headers() {
        let m = parse_message(b"garbage without headers");
        assert!(m.html.is_none());
        let h = parse_header(b"From: <<<\r\nSubject\r\n\r\n");
        assert!(h.subject.is_empty());
    }

    #[test]
    fn snippet_skips_quotes_and_collapses_whitespace() {
        assert_eq!(snippet("Hallo  Welt,\n\n> alt\n  neu   hier", 100), "Hallo Welt, neu hier");
        assert_eq!(snippet("abcdefgh", 3), "abc");
    }

    #[test]
    fn normalizes_subjects() {
        assert_eq!(normalize_subject("AW: Re: WG: Termin"), "Termin");
        assert_eq!(normalize_subject("Retro"), "Retro");
    }
}
