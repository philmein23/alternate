//! One Markdown file per document. The body is the text as it reads on the page. Anything else lives
//! in a trailing HTML comment, which other Markdown apps don't show.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::{Alt, Doc, Level, Source, Target};

const OPEN: &str = "<!-- alternate:data\n";
const CLOSE: &str = "\n-->\n";
const SCHEMA: u32 = 1;

/// Finds text by what it says, so edits made in another app don't scramble it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub quote: String,
    pub occurrence: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct AltMeta {
    text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    ai: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TargetMeta {
    anchor: Anchor,
    level: Level,
    original: String,
    alts: Vec<AltMeta>,
    active: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FileMeta {
    schema: u32,
    #[serde(default)]
    targets: Vec<TargetMeta>,
    #[serde(default)]
    ghosts: Vec<Anchor>,
    #[serde(default)]
    overflow: String,
    /// A comment from an earlier save that couldn't be read, kept so saving can't destroy it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preserved: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The comment was there but couldn't be read. It's kept and written back on save.
    UnreadableExtras,
    /// Saved text no longer in the body, so its alternatives or fading were dropped.
    MissingAnchor(String),
}

#[derive(Debug)]
pub struct Loaded {
    pub doc: Doc,
    pub problems: Vec<Problem>,
    /// The raw comment when it couldn't be read.
    pub preserved: Option<String>,
}

fn nth_start(body: &str, quote: &str, occurrence: usize) -> Option<usize> {
    if quote.is_empty() {
        return None;
    }
    let mut from = 0;
    for seen in 0.. {
        let at = from + body[from..].find(quote)?;
        if seen == occurrence {
            return Some(at);
        }
        from = at + body[at..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

fn anchor_of(body: &str, start: usize, end: usize) -> Anchor {
    let quote = &body[start..end];
    let mut occurrence = 0;
    let mut from = 0;
    while let Some(i) = body[from..].find(quote) {
        let at = from + i;
        if at >= start {
            break;
        }
        occurrence += 1;
        from = at + body[at..].chars().next().map_or(1, char::len_utf8);
    }
    Anchor { quote: quote.to_string(), occurrence }
}

fn meta_of(doc: &Doc, preserved: Option<&str>) -> Option<FileMeta> {
    if doc.targets.is_empty() && doc.ghosts.is_empty() && doc.overflow.is_empty() {
        return None;
    }
    let targets = doc
        .targets
        .iter()
        .map(|t| TargetMeta {
            anchor: anchor_of(&doc.text, t.range.start, t.range.end),
            level: t.level,
            original: t.original.clone(),
            alts: t.alts.iter().map(|a| AltMeta { text: a.text.clone(), ai: a.source == Source::Ai }).collect(),
            active: t.active,
        })
        .collect();
    let ghosts = doc.ghosts.iter().map(|g| anchor_of(&doc.text, g.range.start, g.range.end)).collect();
    Some(FileMeta { schema: SCHEMA, targets, ghosts, overflow: doc.overflow.clone(), preserved: preserved.map(str::to_string) })
}

/// The whole file for `doc`. `preserved` is a comment an earlier load couldn't read.
pub fn serialize(doc: &Doc, preserved: Option<&str>) -> String {
    let Some(meta) = meta_of(doc, preserved) else {
        return match preserved {
            Some(raw) => format!("{}\n\n{raw}", doc.text),
            None => doc.text.clone(),
        };
    };
    let json = serde_json::to_string(&meta).expect("the file data has only strings and numbers").replace("-->", "--\\u003e");
    format!("{}\n\n{OPEN}{json}{CLOSE}", doc.text)
}

/// Where our comment starts, when it is the last thing in the file.
fn comment_start(file: &str) -> Option<usize> {
    let at = file.rfind(OPEN)?;
    let rest = &file[at + OPEN.len()..];
    let end = rest.find("-->")?;
    rest[end + 3..].trim().is_empty().then_some(at)
}

fn body_before(file: &str, at: usize) -> &str {
    let before = &file[..at];
    before.strip_suffix("\n\n").unwrap_or(before)
}

pub fn parse(file: &str) -> Loaded {
    let Some(at) = comment_start(file) else {
        return Loaded { doc: Doc::new(file), problems: Vec::new(), preserved: None };
    };
    let body = body_before(file, at);
    let raw = &file[at..];
    let json = raw[OPEN.len()..].rsplit_once("-->").map_or("", |(j, _)| j.trim());
    let meta = serde_json::from_str::<FileMeta>(json).ok().filter(|m| m.schema == SCHEMA);
    let Some(meta) = meta else {
        return Loaded { doc: Doc::new(body), problems: vec![Problem::UnreadableExtras], preserved: Some(raw.to_string()) };
    };

    let mut doc = Doc::new(body);
    doc.overflow = meta.overflow;
    let mut problems = Vec::new();
    for t in meta.targets {
        let Some(start) = nth_start(body, &t.anchor.quote, t.anchor.occurrence) else {
            problems.push(Problem::MissingAnchor(t.anchor.quote));
            continue;
        };
        if t.alts.is_empty() {
            continue;
        }
        let id = doc.next_id();
        doc.targets.push(Target {
            id,
            range: start..start + t.anchor.quote.len(),
            level: t.level,
            original: t.original,
            active: t.active.min(t.alts.len() - 1),
            alts: t.alts.into_iter().map(|a| Alt { text: a.text, source: if a.ai { Source::Ai } else { Source::Mine } }).collect(),
        });
    }
    for g in meta.ghosts {
        match nth_start(body, &g.quote, g.occurrence) {
            Some(start) => doc.add_ghost(start..start + g.quote.len()),
            None => problems.push(Problem::MissingAnchor(g.quote)),
        }
    }
    Loaded { doc, problems, preserved: meta.preserved }
}

/// Writes to a temporary file beside `path`, then renames it over, so a crash can't leave half a
/// document.
pub fn save_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let temp = path.with_file_name(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    fs::write(&temp, contents)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Doc {
        let mut doc = Doc::new("# Title\n\nThe cat sat near the cat.\n");
        let start = doc.text.rfind("cat").unwrap();
        let id = doc.next_id();
        doc.targets.push(Target {
            id,
            range: start..start + 3,
            level: Level::Word,
            original: "cat".to_string(),
            alts: vec![Alt { text: "cat".to_string(), source: Source::Mine }, Alt { text: "dog".to_string(), source: Source::Ai }],
            active: 0,
        });
        doc.add_ghost(9..12);
        doc.overflow = "spare line\n".to_string();
        doc
    }

    #[test]
    fn a_saved_file_round_trips_byte_for_byte() {
        let first = serialize(&sample(), None);
        let loaded = parse(&first);
        assert!(loaded.problems.is_empty());
        assert_eq!(serialize(&loaded.doc, loaded.preserved.as_deref()), first);
    }

    #[test]
    fn loading_restores_everything() {
        let loaded = parse(&serialize(&sample(), None));
        let doc = sample();
        assert_eq!(loaded.doc.text, doc.text);
        assert_eq!(loaded.doc.ghosts, doc.ghosts);
        assert_eq!(loaded.doc.overflow, doc.overflow);
        assert_eq!(loaded.doc.targets[0].range, doc.targets[0].range);
        assert_eq!(loaded.doc.targets[0].alts, doc.targets[0].alts);
    }

    #[test]
    fn a_body_ending_in_newlines_stays_exact() {
        let mut doc = sample();
        doc.text.push_str("\n\n");
        assert_eq!(parse(&serialize(&doc, None)).doc.text, doc.text);
    }

    #[test]
    fn a_file_with_no_comment_opens_as_a_plain_document() {
        let loaded = parse("just text\n");
        assert_eq!(loaded.doc.text, "just text\n");
        assert!(loaded.doc.targets.is_empty() && loaded.problems.is_empty());
        assert_eq!(serialize(&loaded.doc, None), "just text\n");
    }

    #[test]
    fn a_plain_document_gains_no_comment() {
        assert_eq!(serialize(&Doc::new("words"), None), "words");
    }

    #[test]
    fn an_unreadable_comment_never_loses_the_body_or_itself() {
        let file = format!("my words\n\n{OPEN}not json{CLOSE}");
        let loaded = parse(&file);
        assert_eq!(loaded.doc.text, "my words");
        assert_eq!(loaded.problems, vec![Problem::UnreadableExtras]);
        assert_eq!(serialize(&loaded.doc, loaded.preserved.as_deref()), file);
    }

    #[test]
    fn a_comment_from_a_newer_version_is_kept_too() {
        let file = format!("words\n\n{OPEN}{{\"schema\":99}}{CLOSE}");
        let loaded = parse(&file);
        assert_eq!(loaded.problems, vec![Problem::UnreadableExtras]);
        assert_eq!(serialize(&loaded.doc, loaded.preserved.as_deref()), file);
    }

    #[test]
    fn new_extras_keep_an_unreadable_comment_inside_them() {
        let file = format!("my words\n\n{OPEN}not json{CLOSE}");
        let mut loaded = parse(&file);
        loaded.doc.overflow = "stash".to_string();
        let saved = serialize(&loaded.doc, loaded.preserved.as_deref());
        let again = parse(&saved);
        assert_eq!(again.doc.overflow, "stash");
        assert_eq!(again.preserved.as_deref(), Some(format!("{OPEN}not json{CLOSE}").as_str()));
    }

    #[test]
    fn an_anchor_whose_quote_is_gone_is_dropped_and_reported() {
        let saved = serialize(&sample(), None);
        let edited = saved.replacen("near the cat", "near the mat", 1);
        let loaded = parse(&edited);
        assert_eq!(loaded.problems, vec![Problem::MissingAnchor("cat".to_string())]);
        assert!(loaded.doc.targets.is_empty());
    }

    #[test]
    fn edits_in_another_app_keep_later_anchors_on_their_text() {
        let saved = serialize(&sample(), None);
        let edited = saved.replacen("# Title", "# A much longer title", 1);
        let loaded = parse(&edited);
        assert_eq!(&loaded.doc.text[loaded.doc.targets[0].range.clone()], "cat");
        assert_eq!(loaded.doc.targets[0].range.start, loaded.doc.text.rfind("cat").unwrap());
    }

    #[test]
    fn a_literal_comment_close_in_stored_text_is_escaped() {
        let mut doc = sample();
        doc.overflow = "an arrow --> here".to_string();
        let saved = serialize(&doc, None);
        let comment = &saved[saved.rfind(OPEN).unwrap()..];
        assert_eq!(comment.matches("-->").count(), 1, "only the real close: {comment}");
        assert_eq!(parse(&saved).doc.overflow, "an arrow --> here");
    }

    #[test]
    fn a_comment_earlier_in_the_body_is_left_as_text() {
        let file = format!("{OPEN}x\n-->\nmore writing after it\n");
        let loaded = parse(&file);
        assert_eq!(loaded.doc.text, file);
        assert!(loaded.problems.is_empty());
    }

    #[test]
    fn repeated_text_anchors_to_the_right_occurrence() {
        let body = "aaaa";
        let a = anchor_of(body, 2, 4);
        assert_eq!(a.occurrence, 2);
        assert_eq!(nth_start(body, &a.quote, a.occurrence), Some(2));
    }

    #[test]
    fn saving_replaces_the_file_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("alternate-save-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.md");
        fs::write(&path, "old").unwrap();
        save_atomic(&path, "new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
