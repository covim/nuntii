//! Construction of outgoing messages.

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, Mailboxes, MultiPart, SinglePart};
use lettre::Message;

use crate::error::{AppError, Result};

pub struct OutgoingAttachment {
    pub filename: String,
    pub mime: String,
    pub data: Vec<u8>,
}

pub struct Draft {
    pub from: Mailbox,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    /// Formatted body from the editor; sent as multipart/alternative together with `body_text`.
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<OutgoingAttachment>,
}

pub struct Built {
    pub raw: Vec<u8>,
    pub env_from: String,
    pub env_to: Vec<String>,
}

fn mailboxes(list: &[String]) -> Result<Option<Mailboxes>> {
    let mut out = Mailboxes::new();
    for entry in list.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let mb: Mailbox = entry
            .parse()
            .map_err(|_| AppError::Invalid(format!("Ungültige Empfängeradresse: {entry}")))?;
        out.push(mb);
    }
    Ok((out.iter().next().is_some()).then_some(out))
}

fn angle(id: &str) -> String {
    if id.starts_with('<') {
        id.to_string()
    } else {
        format!("<{id}>")
    }
}

/// Complete HTML document around the editor output, with defaults matching the editor.
fn html_document(body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<style>p{{margin:0}}blockquote{{margin:0 0 0 .25em;padding-left:.75em;border-left:3px solid #ccc;color:#555}}</style></head>\
<body style=\"font-family:Arial,Helvetica,sans-serif;font-size:14px;line-height:1.5\">{body}</body></html>"
    )
}

pub fn build(draft: Draft) -> Result<Built> {
    let to = mailboxes(&draft.to)?;
    let cc = mailboxes(&draft.cc)?;
    let bcc = mailboxes(&draft.bcc)?;
    if to.is_none() && cc.is_none() && bcc.is_none() {
        return Err(AppError::Invalid("Mindestens ein Empfänger ist erforderlich".into()));
    }

    let mut builder = Message::builder()
        .from(draft.from.clone())
        .subject(draft.subject)
        .user_agent("nuntii".into());
    if let Some(to) = to {
        builder = builder.mailbox(lettre::message::header::To::from(to));
    }
    if let Some(cc) = cc {
        builder = builder.mailbox(lettre::message::header::Cc::from(cc));
    }
    if let Some(bcc) = bcc {
        builder = builder.mailbox(lettre::message::header::Bcc::from(bcc));
    }
    if let Some(irt) = &draft.in_reply_to {
        builder = builder.in_reply_to(angle(irt));
    }
    if !draft.references.is_empty() {
        let refs: Vec<String> = draft.references.iter().map(|r| angle(r)).collect();
        builder = builder.references(refs.join(" "));
    }

    let text = SinglePart::plain(draft.body_text);
    let body = draft.body_html.map(|html| {
        MultiPart::alternative()
            .singlepart(text.clone())
            .singlepart(SinglePart::html(html_document(&html)))
    });
    let message = if draft.attachments.is_empty() {
        match body {
            Some(alternative) => builder.multipart(alternative)?,
            None => builder.singlepart(text)?,
        }
    } else {
        let mut mixed = match body {
            Some(alternative) => MultiPart::mixed().multipart(alternative),
            None => MultiPart::mixed().singlepart(text),
        };
        for att in draft.attachments {
            let ct = ContentType::parse(&att.mime)
                .unwrap_or_else(|_| ContentType::parse("application/octet-stream").unwrap());
            mixed = mixed.singlepart(Attachment::new(att.filename).body(att.data, ct));
        }
        builder.multipart(mixed)?
    };

    let envelope = message.envelope();
    Ok(Built {
        env_from: envelope
            .from()
            .map(|a| a.to_string())
            .unwrap_or_else(|| draft.from.email.to_string()),
        env_to: envelope.to().iter().map(|a| a.to_string()).collect(),
        // `formatted()` omits Bcc, so blind recipients stay hidden.
        raw: message.formatted(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mime::parse::{parse_header, parse_message};

    #[test]
    fn builds_reply_with_attachment_and_hidden_bcc() {
        let built = build(Draft {
            from: "Ich <me@example.de>".parse().unwrap(),
            to: vec!["Anna <anna@example.de>".into()],
            cc: vec![],
            bcc: vec!["secret@example.de".into()],
            subject: "Re: Angebot".into(),
            body_text: "Danke, passt!\n\n-- \nIch".into(),
            body_html: Some("<p><strong>Danke</strong>, passt!</p>".into()),
            in_reply_to: Some("m1@example.de".into()),
            references: vec!["m0@example.de".into(), "m1@example.de".into()],
            attachments: vec![OutgoingAttachment {
                filename: "notiz.txt".into(),
                mime: "text/plain".into(),
                data: b"hallo".to_vec(),
            }],
        })
        .unwrap();

        assert_eq!(built.env_from, "me@example.de");
        assert!(built.env_to.contains(&"secret@example.de".to_string()));
        let raw = String::from_utf8_lossy(&built.raw);
        assert!(!raw.contains("secret@example.de"), "Bcc must not leak into headers");

        let parsed = parse_message(&built.raw);
        let header = parse_header(&built.raw);
        assert_eq!(header.in_reply_to.as_deref(), Some("m1@example.de"));
        assert_eq!(header.references.len(), 2);
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].filename, "notiz.txt");
        assert!(parsed.text.unwrap().contains("Danke, passt!"));
        assert!(parsed.html.unwrap().contains("<strong>Danke</strong>"));
    }

    #[test]
    fn rejects_missing_or_invalid_recipients() {
        let base = || Draft {
            from: "me@example.de".parse().unwrap(),
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: "x".into(),
            body_text: "x".into(),
            body_html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };
        assert!(build(base()).is_err());
        let mut d = base();
        d.to = vec!["kein-at-zeichen".into()];
        assert!(build(d).is_err());
    }
}
