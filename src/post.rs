//! Post to X: opens X's own compose page with the text filled in. The app holds no X login and
//! posts nothing itself.

use rune::Tx;

use crate::State;
use crate::model::Doc;

pub const LIMIT: usize = 280;

/// The page as a reader would have it, minus anything ghosted.
fn without_ghosts(doc: &Doc) -> String {
    let mut text = doc.text.clone();
    for ghost in doc.ghosts.iter().rev() {
        if ghost.range.end <= text.len() {
            text.replace_range(ghost.range.clone(), "");
        }
    }
    text
}

fn strip_link(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let found = rest[open..].find("](").map(|i| open + i).and_then(|mid| rest[mid..].find(')').map(|end| (mid, mid + end)));
        let Some((mid, end)) = found else { break };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open + 1..mid]);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Takes off Markdown marks, leaving the words.
pub fn plain(markdown: &str) -> String {
    let lines: Vec<String> = markdown
        .lines()
        .map(|line| {
            let t = line.trim_start();
            let t = t.trim_start_matches('#').trim_start();
            let t = t.strip_prefix("> ").unwrap_or(t);
            let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t);
            strip_link(t).replace("**", "").replace("__", "").replace('`', "")
        })
        .collect();
    let joined = lines.join("\n");
    let mut out = String::with_capacity(joined.len());
    for part in joined.split("\n\n") {
        let part = part.trim();
        if !part.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(part);
        }
    }
    out
}

pub fn post_text(doc: &Doc) -> String {
    let squeezed = without_ghosts(doc);
    let tidied = squeezed.split(' ').filter(|w| !w.is_empty() || squeezed.is_empty()).collect::<Vec<_>>().join(" ");
    plain(&tidied)
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn intent_url(text: &str) -> String {
    format!("https://x.com/intent/post?text={}", encode(text))
}

impl State {
    pub fn post_to_x(&mut self, tx: &mut Tx) {
        let text = post_text(&self.doc);
        if text.is_empty() {
            self.file.notice = Some("There's nothing to post yet.".to_string());
            return;
        }
        let count = text.chars().count();
        self.file.notice = match tx.open_url(&intent_url(&text)) {
            Err(_) => Some("Couldn't open your browser.".to_string()),
            Ok(()) if count > LIMIT => Some(format!("That's {count} characters. X's limit is {LIMIT} on a standard account, so it may need trimming.")),
            Ok(()) => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghosted_text_is_left_out() {
        let mut d = Doc::new("keep this drop that end");
        d.add_ghost(10..20);
        assert_eq!(post_text(&d), "keep this end");
    }

    #[test]
    fn markdown_marks_come_off() {
        assert_eq!(plain("# Title\n\nSome **bold** and `code` and [a link](https://x.y)."), "Title\n\nSome bold and code and a link.");
        assert_eq!(plain("> quoted\n- item"), "quoted\nitem");
    }

    #[test]
    fn the_hidden_data_comment_is_never_in_the_text_posted() {
        let mut d = Doc::new("hello");
        d.overflow = "secret stash".to_string();
        let saved = crate::file::serialize(&d, None);
        assert!(saved.contains("alternate:data"));
        let back = crate::file::parse(&saved).doc;
        let post = post_text(&back);
        assert_eq!(post, "hello");
    }

    #[test]
    fn quotes_and_emoji_are_encoded() {
        let url = intent_url("she said \"hi\" & left \u{1f600}");
        assert_eq!(url, "https://x.com/intent/post?text=she%20said%20%22hi%22%20%26%20left%20%F0%9F%98%80");
    }

    #[test]
    fn a_link_with_no_close_is_left_alone() {
        assert_eq!(plain("see [this"), "see [this");
    }
}
