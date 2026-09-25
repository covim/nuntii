//! HTML mail sanitizing. The output is rendered in a sandboxed iframe (no scripts) with a CSP,
//! but it must be safe on its own: no scripts, event handlers, forms, or remote loads unless the
//! user allowed remote content for this mail.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ammonia::{Builder, UrlRelative};

pub struct Sanitized {
    pub html: String,
    /// Number of remote resources that were blocked (drives the "load images" banner).
    pub blocked_remote: usize,
}

fn is_remote(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("http:") || u.starts_with("https:") || u.starts_with("//")
}

/// `inline_images` maps Content-IDs (without angle brackets) to `data:` URIs.
pub fn sanitize(html: &str, inline_images: &HashMap<String, String>, allow_remote: bool) -> Sanitized {
    let blocked = Arc::new(AtomicUsize::new(0));
    let counter = blocked.clone();
    let cids = Arc::new(inline_images.clone());

    let generic: HashSet<&str> = [
        "style", "align", "valign", "width", "height", "bgcolor", "color", "border", "dir", "lang",
        "title", "cellpadding", "cellspacing", "colspan", "rowspan", "face", "size",
    ]
    .into_iter()
    .collect();
    let extra_tags: HashSet<&str> = ["center", "font", "span", "div", "u", "s"].into_iter().collect();
    let schemes: HashSet<&str> = ["http", "https", "mailto", "tel", "data", "cid"].into_iter().collect();

    let cleaned = Builder::default()
        .add_tags(extra_tags)
        .add_generic_attributes(generic)
        .url_schemes(schemes)
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(move |element, attribute, value| {
            match (element, attribute) {
                ("img", "src") => {
                    if let Some(cid) = value.strip_prefix("cid:") {
                        return cids.get(cid).map(|d| Cow::Owned(d.clone()));
                    }
                    if value.starts_with("data:image/") {
                        return Some(Cow::Borrowed(value));
                    }
                    if is_remote(value) && allow_remote {
                        return Some(Cow::Borrowed(value));
                    }
                    counter.fetch_add(1, Ordering::Relaxed);
                    None
                }
                (_, "style") => {
                    // CSS can load resources via url(), e.g. background images used as trackers.
                    let lower = value.to_ascii_lowercase();
                    if (lower.contains("url(") || lower.contains("@import") || lower.contains("expression("))
                        && !allow_remote
                    {
                        counter.fetch_add(1, Ordering::Relaxed);
                        None
                    } else {
                        Some(Cow::Borrowed(value))
                    }
                }
                // Data URIs are only acceptable as image sources.
                (_, "href") if value.trim_start().to_ascii_lowercase().starts_with("data:") => None,
                _ => Some(Cow::Borrowed(value)),
            }
        })
        .clean(html)
        .to_string();

    Sanitized {
        html: cleaned,
        blocked_remote: blocked.load(Ordering::Relaxed),
    }
}

/// Wraps sanitized HTML in a document with a restrictive CSP for the `srcdoc` iframe.
pub fn wrap_document(body: &str, allow_remote: bool) -> String {
    let img_src = if allow_remote { "data: https: http:" } else { "data:" };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src {img_src}; style-src 'unsafe-inline'; font-src data:\">\
<base target=\"_blank\">\
<style>html{{overflow-y:hidden;overflow-x:auto}}body{{margin:0;padding:16px;font-family:system-ui,-apple-system,'Segoe UI',sans-serif;font-size:14px;line-height:1.5;color:#1a1a1a;background:#fff;word-wrap:break-word}}img{{max-width:100%;height:auto}}blockquote{{margin:0 0 0 .5em;padding-left:.75em;border-left:3px solid #ccc;color:#555}}pre{{white-space:pre-wrap}}</style>\
</head><body>{body}</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str, allow: bool) -> Sanitized {
        let mut cids = HashMap::new();
        cids.insert("logo@x".to_string(), "data:image/png;base64,AAAA".to_string());
        sanitize(html, &cids, allow)
    }

    #[test]
    fn strips_scripts_and_handlers() {
        let s = clean(r#"<p onclick="evil()">hi</p><script>alert(1)</script><img src="x" onerror="evil()">"#, false);
        assert!(!s.html.contains("script"));
        assert!(!s.html.contains("onclick"));
        assert!(!s.html.contains("onerror"));
        assert!(s.html.contains("<p>hi</p>"));
    }

    #[test]
    fn blocks_tracking_pixels_unless_allowed() {
        let html = r#"<img src="https://t.example/p.gif" width="1" height="1"><img src="http://a.example/b.png">"#;
        let blocked = clean(html, false);
        assert!(!blocked.html.contains("t.example"));
        assert_eq!(blocked.blocked_remote, 2);
        let allowed = clean(html, true);
        assert!(allowed.html.contains("https://t.example/p.gif"));
        assert_eq!(allowed.blocked_remote, 0);
    }

    #[test]
    fn blocks_css_url_loads() {
        let s = clean(r#"<div style="background:url(https://t.example/x)">a</div>"#, false);
        assert!(!s.html.contains("t.example"));
        assert_eq!(s.blocked_remote, 1);
        let ok = clean(r#"<div style="color:red">a</div>"#, false);
        assert!(ok.html.contains("color:red"));
    }

    #[test]
    fn resolves_cid_images() {
        let s = clean(r#"<img src="cid:logo@x"><img src="cid:unknown">"#, false);
        assert!(s.html.contains("data:image/png;base64,AAAA"));
        assert!(!s.html.contains("cid:"));
    }

    #[test]
    fn removes_forms_iframes_and_js_links() {
        let s = clean(
            r#"<form action="https://x"><input name="pw"></form><iframe src="https://x"></iframe><a href="javascript:alert(1)">x</a><a href="https://ok.example">ok</a>"#,
            true,
        );
        assert!(!s.html.contains("<form"));
        assert!(!s.html.contains("<input"));
        assert!(!s.html.contains("<iframe"));
        assert!(!s.html.contains("javascript:"));
        assert!(s.html.contains(r#"href="https://ok.example""#));
        assert!(s.html.contains("noopener"));
    }
}
