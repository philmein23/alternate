//! Vim on the page: modes, operators, motions, text objects, registers, repeat, macros, marks,
//! search and `:` commands. Off by default, and always starts in Insert so typing works.

mod command;
pub mod motion;

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rune::{Key, KeyEvent, Mods, TextHighlight, Tx};

use self::motion::Target;
use crate::State;
use crate::rebase::Edit;
use crate::ui::theme::THEME;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Insert,
    Normal,
    Visual,
    VisualLine,
    Replace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Delete,
    Change,
    Yank,
    Lower,
    Upper,
    Toggle,
    Indent,
    Outdent,
}

impl Op {
    fn label(self) -> &'static str {
        match self {
            Op::Delete => "d",
            Op::Change => "c",
            Op::Yank => "y",
            Op::Lower => "gu",
            Op::Upper => "gU",
            Op::Toggle => "g~",
            Op::Indent => ">",
            Op::Outdent => "<",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Pending {
    #[default]
    None,
    G,
    Find(char),
    Replace,
    SetMark,
    Jump { line: bool },
    Object { inner: bool },
    Register,
    MacroStore,
    MacroPlay,
}

#[derive(Clone, Debug)]
struct Reg {
    text: String,
    linewise: bool,
}

#[derive(Clone, Debug)]
struct Cmdline {
    kind: char,
    text: String,
}

#[derive(Clone, Debug)]
struct Search {
    pattern: String,
    forward: bool,
    whole: bool,
}

const ON_FILE: &str = "vim-on";
/// Replayed keys arrive one per frame, so a repeat is kept to about half a minute.
const MAX_REPLAY: usize = 2000;
const INDENT: &str = "    ";

#[derive(Debug, Default)]
pub struct Vim {
    pub on: bool,
    pub mode: Mode,
    count: u32,
    op_count: u32,
    op: Option<Op>,
    pending: Pending,
    register: Option<char>,
    registers: HashMap<char, Reg>,
    last_find: Option<(char, char)>,
    search: Option<Search>,
    cmdline: Option<Cmdline>,
    visual_anchor: Option<usize>,
    marks: HashMap<char, usize>,
    macros: HashMap<char, Vec<KeyEvent>>,
    macro_reg: Option<char>,
    macro_buf: Vec<KeyEvent>,
    last_macro: Option<char>,
    change_keys: Vec<KeyEvent>,
    last_change: Vec<KeyEvent>,
    changed: bool,
    queue: VecDeque<KeyEvent>,
    /// The replayed key on its way, to tell it apart from one typed meanwhile.
    expected: Option<KeyEvent>,
    replaying: bool,
    /// Arrow keys sent on the way, so they aren't taken for typed ones.
    synthetic: usize,
    dir: Option<PathBuf>,
}

impl Vim {
    pub fn load(dir: &Path) -> Vim {
        Vim { on: dir.join(ON_FILE).exists(), dir: Some(dir.to_path_buf()), ..Vim::default() }
    }

    pub fn toggle(&mut self) {
        self.on = !self.on;
        self.mode = Mode::Insert;
        self.visual_anchor = None;
        self.reset();
        if let Some(path) = self.dir.as_ref().map(|d| d.join(ON_FILE)) {
            let _ = if self.on { fs::write(path, b"") } else { fs::remove_file(path) };
        }
    }

    /// A new entry: typing works straight away, and marks into the old text are gone.
    pub fn start_over(&mut self) {
        self.mode = Mode::Insert;
        self.visual_anchor = None;
        self.marks.clear();
        self.cmdline = None;
        self.queue.clear();
        self.expected = None;
        self.replaying = false;
        self.change_keys.clear();
        self.changed = false;
        self.reset();
    }

    fn reset(&mut self) {
        self.count = 0;
        self.op_count = 0;
        self.op = None;
        self.pending = Pending::None;
        self.register = None;
    }

    fn idle(&self) -> bool {
        self.count == 0 && self.op_count == 0 && self.op.is_none() && self.pending == Pending::None && self.register.is_none() && self.cmdline.is_none()
    }

    fn has_count(&self) -> bool {
        self.count > 0 || self.op_count > 0
    }

    /// The count typed before the operator times the one typed after it.
    fn total(&self) -> usize {
        self.count.max(1) as usize * self.op_count.max(1) as usize
    }

    /// Whether keys are still on their way from a repeat or a motion.
    pub fn busy(&self) -> bool {
        self.replaying || self.synthetic > 0
    }

    fn visual(&self) -> bool {
        self.visual_anchor.is_some()
    }

    fn record(&mut self, ev: &KeyEvent) {
        if self.replaying {
            return;
        }
        if self.macro_reg.is_some() {
            self.macro_buf.push(ev.clone());
        }
        if self.mode == Mode::Normal && self.idle() {
            self.change_keys.clear();
        }
        self.change_keys.push(ev.clone());
    }

    /// A change is complete once we're back in Normal mode with nothing half typed.
    fn finish(&mut self) {
        if self.changed && self.mode == Mode::Normal && self.idle() {
            if !self.replaying {
                self.last_change = self.change_keys.clone();
            }
            self.changed = false;
        }
    }

    fn store(&mut self, text: String, linewise: bool) {
        let chosen = self.register;
        if chosen == Some('_') {
            return;
        }
        let reg = Reg { text, linewise };
        if let Some(c) = chosen {
            self.registers.insert(c, reg.clone());
        }
        self.registers.insert('"', reg);
    }

    /// What the bottom bar shows.
    pub fn status(&self) -> String {
        if let Some(line) = &self.cmdline {
            return format!("{}{}", line.kind, line.text);
        }
        let mode = match self.mode {
            Mode::Insert => "INSERT",
            Mode::Normal => "NORMAL",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::Replace => "REPLACE",
        };
        let mut out = mode.to_string();
        if self.has_count() || self.op.is_some() {
            out.push(' ');
            if self.op_count > 0 {
                out.push_str(&self.op_count.to_string());
            }
            if let Some(op) = self.op {
                out.push_str(op.label());
            }
            if self.count > 0 {
                out.push_str(&self.count.to_string());
            }
        }
        if let Some(r) = self.macro_reg {
            out.push_str(&format!(" \u{b7} recording @{r}"));
        }
        out
    }

    /// Keeps marks in place as the text changes.
    pub fn rebase_marks(&mut self, edit: Edit) {
        for at in self.marks.values_mut() {
            if *at >= edit.start + edit.removed {
                *at = *at + edit.inserted - edit.removed;
            } else if *at > edit.start {
                *at = edit.start;
            }
        }
    }
}

fn plain_key(key: Key) -> KeyEvent {
    KeyEvent { key, mods: Mods::default() }
}

fn with_cmd(key: Key) -> KeyEvent {
    KeyEvent { key, mods: Mods { cmd: true, ..Mods::default() } }
}

fn is_arrow(ev: &KeyEvent) -> bool {
    matches!(ev.key, Key::Left | Key::Right | Key::Up | Key::Down)
}

fn toggle_case(text: &str) -> String {
    text.chars().flat_map(|c| if c.is_lowercase() { c.to_uppercase().collect::<Vec<_>>() } else { c.to_lowercase().collect() }).collect()
}

/// The stretch being selected: start, exclusive end, and whether it means whole lines.
fn visual_span(text: &str, anchor: usize, focus: usize, line: bool) -> (usize, usize, bool) {
    let (a, b) = (anchor.min(focus), anchor.max(focus));
    if line { (motion::line_start(text, a), motion::line_end(text, b), true) } else { (a, motion::next_char(text, b), false) }
}

/// Selection and block cursor, drawn on the text.
pub fn highlights(s: &State) -> Vec<TextHighlight> {
    if !s.vim.on || matches!(s.vim.mode, Mode::Insert | Mode::Replace) {
        return Vec::new();
    }
    let text = &s.edit.text;
    let at = s.edit.focus.min(text.len());
    let mut out = Vec::new();
    if let Some(anchor) = s.vim.visual_anchor {
        let (a, b, _) = visual_span(text, anchor.min(text.len()), at, s.vim.mode == Mode::VisualLine);
        out.push(TextHighlight::new(a..b, THEME.accent.alpha(0.3)));
    }
    if block_visible(s) {
        out.push(TextHighlight::new(at..motion::next_char(text, at), THEME.accent.alpha(0.55)));
    }
    out
}

/// Whether a character is under the cursor to draw the block on. If not, the bar stays.
pub fn block_visible(s: &State) -> bool {
    s.vim.on && !matches!(s.vim.mode, Mode::Insert | Mode::Replace) && s.edit.text[s.edit.focus.min(s.edit.text.len())..].chars().next().is_some_and(|c| c != '\n')
}

impl State {
    pub(self) fn vim_send(&mut self, ev: KeyEvent, tx: &mut Tx) {
        self.vim.synthetic += 1;
        tx.send_key(ev);
    }

    /// Goes to `target` with one real arrow key at the end, so the page scrolls to the cursor.
    pub(self) fn vim_jump(&mut self, target: usize, tx: &mut Tx) {
        let target = motion::clamp(&self.edit.text, target);
        let text = &self.edit.text;
        let (near, arrow) = if target > 0 {
            (motion::prev_char(text, target), Key::Right)
        } else if !text.is_empty() {
            (motion::next_char(text, 0), Key::Left)
        } else {
            return;
        };
        self.edit.set_selection(near, near);
        self.vim_send(plain_key(arrow), tx);
    }

    fn vim_arrows(&mut self, key: Key, n: usize, tx: &mut Tx) {
        for _ in 0..n.min(self.edit.text.len().max(1)) {
            self.vim_send(plain_key(key.clone()), tx);
        }
    }

    fn vim_caret(&mut self, at: usize) {
        self.edit.set_selection(at, at);
    }

    fn vim_edit(&mut self, a: usize, b: usize, with: &str) {
        self.edit.break_history_group();
        self.edit.set_selection(a, b);
        self.edit.insert(with);
        self.edit.break_history_group();
        self.sync_field();
        self.vim.changed = true;
    }

    fn vim_enter_insert(&mut self, mode: Mode) {
        self.vim.mode = mode;
        self.vim.visual_anchor = None;
        self.vim.changed = true;
        self.vim.reset();
        self.edit.break_history_group();
    }

    fn vim_leave_insert(&mut self) {
        let at = self.edit.focus;
        if at > motion::line_start(&self.edit.text, at) {
            self.vim_caret(motion::prev_char(&self.edit.text, at));
        }
        self.vim.mode = Mode::Normal;
        self.vim.reset();
        self.edit.break_history_group();
    }

    fn vim_escape(&mut self) {
        self.vim.visual_anchor = None;
        self.vim.mode = Mode::Normal;
        self.vim.reset();
    }

    fn vim_apply(&mut self, op: Op, a: usize, b: usize, linewise: bool, tx: &mut Tx) {
        let text = self.edit.text.clone();
        let (a, b) = if linewise { (motion::line_start(&text, a), motion::line_end(&text, b)) } else { (a, b) };
        if a == b && !linewise && op != Op::Change {
            return self.vim.reset();
        }
        let body = text[a..b].to_string();
        let yanked = if linewise { format!("{body}\n") } else { body.clone() };
        match op {
            Op::Yank => {
                self.vim.store(yanked, linewise);
                self.vim_jump(a, tx);
            }
            Op::Delete => {
                self.vim.store(yanked, linewise);
                let (from, to) = match (linewise, b < text.len(), a > 0) {
                    (true, true, _) => (a, b + 1),
                    (true, false, true) => (a - 1, b),
                    _ => (a, b),
                };
                self.vim_edit(from, to, "");
                let at = from.min(self.edit.text.len());
                let at = if linewise { motion::first_nonblank(&self.edit.text, at) } else { at };
                self.vim_jump(at, tx);
            }
            Op::Change => {
                self.vim.store(yanked, linewise);
                self.vim_edit(a, b, "");
                self.vim_caret(a);
                self.vim_enter_insert(Mode::Insert);
                return;
            }
            Op::Lower | Op::Upper | Op::Toggle => {
                let with = match op {
                    Op::Lower => body.to_lowercase(),
                    Op::Upper => body.to_uppercase(),
                    _ => toggle_case(&body),
                };
                self.vim_edit(a, b, &with);
                self.vim_jump(a, tx);
            }
            Op::Indent | Op::Outdent => {
                let with: Vec<String> = body
                    .split('\n')
                    .map(|l| match op {
                        Op::Indent if l.is_empty() => String::new(),
                        Op::Indent => format!("{INDENT}{l}"),
                        _ => l.strip_prefix(INDENT).or_else(|| l.strip_prefix('\t')).unwrap_or_else(|| l.trim_start_matches(' ')).to_string(),
                    })
                    .collect();
                self.vim_edit(a, b, &with.join("\n"));
                let at = motion::first_nonblank(&self.edit.text, a);
                self.vim_jump(at, tx);
            }
        }
        self.vim.reset();
    }

    /// Either moves there, or hands the stretch to the waiting operator.
    pub(self) fn vim_carry_out(&mut self, t: Target, tx: &mut Tx) {
        let at = self.edit.focus;
        match self.vim.op.take() {
            Some(op) => {
                let (a, b) = (at.min(t.to), at.max(t.to));
                let b = if t.inclusive { motion::next_char(&self.edit.text, b) } else { b };
                self.vim_apply(op, a, b, t.linewise, tx);
            }
            None => {
                self.vim.reset();
                self.vim_jump(t.to, tx);
            }
        }
    }

    fn vim_line_op(&mut self, op: Op, tx: &mut Tx) {
        let n = self.vim.total();
        let at = self.edit.focus;
        let last = motion::line_offset(&self.edit.text, at, n as isize - 1);
        self.vim.op = None;
        self.vim_apply(op, at, last, true, tx);
    }

    fn vim_op_key(&mut self, op: Op, tx: &mut Tx) {
        if self.vim.op == Some(op) {
            return self.vim_line_op(op, tx);
        }
        self.vim.op = Some(op);
        self.vim.op_count = self.vim.count;
        self.vim.count = 0;
    }

    fn vim_motion(&mut self, c: char, tx: &mut Tx) -> bool {
        let at = self.edit.focus;
        let with_op = self.vim.op.is_some();
        let has_count = self.vim.has_count();
        let n = self.vim.total();
        let on_word = self.edit.text[at..].chars().next().is_some_and(|c| !c.is_whitespace());
        // `cw` on a word means up to its end, as in Vim.
        let c = match c {
            'w' if self.vim.op == Some(Op::Change) && on_word => 'e',
            'W' if self.vim.op == Some(Op::Change) && on_word => 'E',
            other => other,
        };
        let text = &self.edit.text;
        let target = match c {
            'j' | 'k' if with_op => {
                let line = motion::line_offset(text, at, if c == 'j' { n as isize } else { -(n as isize) });
                Some(motion::lines(line))
            }
            'j' | 'k' => {
                self.vim.reset();
                self.vim_arrows(if c == 'j' { Key::Down } else { Key::Up }, n, tx);
                return true;
            }
            'G' => {
                let line = if has_count { motion::line_number(text, n) } else { motion::last_line(text) };
                Some(motion::lines(motion::first_nonblank(text, line)))
            }
            'n' | 'N' => {
                self.vim_search_step(c == 'n', tx);
                return true;
            }
            ';' | ',' => {
                let Some((kind, wanted)) = self.vim.last_find else {
                    self.vim.reset();
                    return true;
                };
                let kind = if c == ',' { swap_find(kind) } else { kind };
                motion::find_char(text, at, kind, wanted, n)
            }
            _ => motion::simple(text, at, c, n, with_op),
        };
        if !matches!(c, 'h' | 'l' | 'j' | 'k' | '0' | '^' | '$' | 'w' | 'W' | 'b' | 'B' | 'e' | 'E' | '}' | '{' | ')' | '(' | '%' | '+' | '-' | '_' | 'G' | ';' | ',') {
            return false;
        }
        match target {
            Some(t) => self.vim_carry_out(t, tx),
            None => self.vim.reset(),
        }
        true
    }

    fn vim_visible_edge(&mut self, right: bool, tx: &mut Tx) {
        self.vim.reset();
        self.vim_send(with_cmd(if right { Key::Right } else { Key::Left }), tx);
    }

    fn vim_visual_apply(&mut self, op: Op, tx: &mut Tx) {
        let Some(anchor) = self.vim.visual_anchor.take() else { return };
        let (a, b, line) = visual_span(&self.edit.text, anchor, self.edit.focus, self.vim.mode == Mode::VisualLine);
        self.vim.mode = Mode::Normal;
        self.vim_apply(op, a, b, line, tx);
    }

    fn vim_join(&mut self, n: usize, tx: &mut Tx) {
        let mut at = self.edit.focus;
        for _ in 0..n.max(1) {
            let end = motion::line_end(&self.edit.text, at);
            if end == self.edit.text.len() {
                break;
            }
            let next = end + 1;
            let lead = self.edit.text[next..].len() - self.edit.text[next..].trim_start_matches([' ', '\t']).len();
            let joins_empty = self.edit.text[next + lead..].starts_with('\n') || next + lead == self.edit.text.len();
            self.vim_edit(end, next + lead, if joins_empty { "" } else { " " });
            at = end;
        }
        self.vim.reset();
        self.vim_jump(at, tx);
    }

    fn vim_visual_key(&mut self, c: char, tx: &mut Tx) -> bool {
        match c {
            'd' | 'x' | 'X' | 'D' => self.vim_visual_apply(Op::Delete, tx),
            'c' | 's' | 'S' | 'C' => self.vim_visual_apply(Op::Change, tx),
            'y' | 'Y' => self.vim_visual_apply(Op::Yank, tx),
            'u' => self.vim_visual_apply(Op::Lower, tx),
            'U' => self.vim_visual_apply(Op::Upper, tx),
            '~' => self.vim_visual_apply(Op::Toggle, tx),
            '>' => self.vim_visual_apply(Op::Indent, tx),
            '<' => self.vim_visual_apply(Op::Outdent, tx),
            'J' => {
                let anchor = self.vim.visual_anchor.take().unwrap_or(self.edit.focus);
                let (a, b) = (anchor.min(self.edit.focus), anchor.max(self.edit.focus));
                let joins = self.edit.text[a..b].matches('\n').count().max(1);
                self.vim.mode = Mode::Normal;
                self.vim_caret(a);
                self.vim_join(joins, tx);
            }
            'p' | 'P' => {
                let Some(reg) = self.vim.registers.get(&self.vim.register.unwrap_or('"')).cloned() else {
                    self.vim_escape();
                    return true;
                };
                let anchor = self.vim.visual_anchor.take().unwrap_or(self.edit.focus);
                let (a, b, _) = visual_span(&self.edit.text, anchor, self.edit.focus, self.vim.mode == Mode::VisualLine);
                self.vim.mode = Mode::Normal;
                self.vim_edit(a, b, reg.text.trim_end_matches(if reg.linewise { "\n" } else { "\0" }));
                self.vim.reset();
                self.vim_jump(a, tx);
            }
            'o' => {
                if let Some(anchor) = self.vim.visual_anchor.replace(self.edit.focus) {
                    self.vim_jump(anchor, tx);
                }
            }
            'v' | 'V' => {
                let target = if c == 'v' { Mode::Visual } else { Mode::VisualLine };
                if self.vim.mode == target {
                    self.vim_escape();
                } else {
                    self.vim.mode = target;
                }
            }
            'i' => self.vim.pending = Pending::Object { inner: true },
            'a' => self.vim.pending = Pending::Object { inner: false },
            _ => return false,
        }
        true
    }

    fn vim_paste(&mut self, after: bool, tx: &mut Tx) {
        let n = self.vim.total();
        let Some(reg) = self.vim.registers.get(&self.vim.register.unwrap_or('"')).cloned() else { return self.vim.reset() };
        let body = reg.text.repeat(n);
        let at = self.edit.focus;
        if reg.linewise {
            let (pos, with, line) = if after {
                let end = motion::line_end(&self.edit.text, at);
                (end, format!("\n{}", body.trim_end_matches('\n')), end + 1)
            } else {
                let start = motion::line_start(&self.edit.text, at);
                (start, body, start)
            };
            self.vim_edit(pos, pos, &with);
            let to = motion::first_nonblank(&self.edit.text, line);
            self.vim.reset();
            self.vim_jump(to, tx);
        } else {
            let on_char = self.edit.text[at..].chars().next().is_some_and(|c| c != '\n');
            let pos = if after && on_char { motion::next_char(&self.edit.text, at) } else { at };
            self.vim_edit(pos, pos, &body);
            let to = motion::prev_char(&self.edit.text, pos + body.len());
            self.vim.reset();
            self.vim_jump(to, tx);
        }
    }

    fn vim_replace_chars(&mut self, c: char, tx: &mut Tx) {
        let n = self.vim.total();
        let at = self.edit.focus;
        let end = motion::line_end(&self.edit.text, at);
        let mut to = at;
        for _ in 0..n {
            if to >= end {
                return self.vim.reset();
            }
            to = motion::next_char(&self.edit.text, to);
        }
        self.vim_edit(at, to, &c.to_string().repeat(n));
        let last = at + c.len_utf8() * (n - 1);
        self.vim.reset();
        self.vim_jump(last, tx);
    }

    fn vim_toggle_chars(&mut self, tx: &mut Tx) {
        let n = self.vim.total();
        let at = self.edit.focus;
        let end = motion::line_end(&self.edit.text, at);
        let mut to = at;
        for _ in 0..n {
            if to < end {
                to = motion::next_char(&self.edit.text, to);
            }
        }
        if to > at {
            let with = toggle_case(&self.edit.text[at..to]);
            self.vim_edit(at, to, &with);
            self.vim.reset();
            let after = (at + with.len()).min(motion::line_end(&self.edit.text, at));
            self.vim_jump(after, tx);
        } else {
            self.vim.reset();
        }
    }

    fn vim_open_line(&mut self, below: bool) {
        let at = self.edit.focus;
        let (pos, caret) = if below {
            let end = motion::line_end(&self.edit.text, at);
            (end, end + 1)
        } else {
            let start = motion::line_start(&self.edit.text, at);
            (start, start)
        };
        self.vim_edit(pos, pos, "\n");
        self.vim_caret(caret);
        self.vim_enter_insert(Mode::Insert);
    }

    fn vim_play(&mut self, keys: Vec<KeyEvent>, n: usize) {
        if keys.is_empty() {
            return;
        }
        let times = n.max(1).min(MAX_REPLAY / keys.len());
        if times < n {
            self.file.notice = Some("That repeat is too long, so it was shortened.".to_string());
        }
        for _ in 0..times {
            for k in keys.iter().rev() {
                self.vim.queue.push_front(k.clone());
            }
        }
        self.vim.replaying = true;
    }

    fn vim_undo(&mut self, redo: bool) {
        let n = self.vim.total();
        self.vim.reset();
        for _ in 0..n {
            if redo {
                self.redo();
            } else {
                self.undo();
            }
        }
        let at = motion::clamp(&self.edit.text, self.edit.focus);
        self.vim_caret(at);
    }

    fn vim_pending(&mut self, pending: Pending, c: char, tx: &mut Tx) {
        match pending {
            Pending::None => {}
            Pending::G => match c {
                'g' => {
                    let n = self.vim.total();
                    let text = &self.edit.text;
                    let to = motion::first_nonblank(text, motion::line_number(text, n));
                    self.vim_carry_out(motion::lines(to), tx);
                }
                'e' | 'E' => {
                    let n = self.vim.total();
                    let mut at = self.edit.focus;
                    for _ in 0..n {
                        at = motion::prev_word_end(&self.edit.text, at, c == 'E');
                    }
                    self.vim_carry_out(Target { to: at, inclusive: true, linewise: false }, tx);
                }
                'j' | 'k' => {
                    let n = self.vim.total();
                    self.vim.reset();
                    self.vim_arrows(if c == 'j' { Key::Down } else { Key::Up }, n, tx);
                }
                '0' | '^' => self.vim_visible_edge(false, tx),
                '$' => self.vim_visible_edge(true, tx),
                'u' => self.vim_op_key(Op::Lower, tx),
                'U' => self.vim_op_key(Op::Upper, tx),
                '~' => self.vim_op_key(Op::Toggle, tx),
                _ => self.vim.reset(),
            },
            Pending::Find(kind) => {
                self.vim.last_find = Some((kind, c));
                let n = self.vim.total();
                match motion::find_char(&self.edit.text, self.edit.focus, kind, c, n) {
                    Some(t) => self.vim_carry_out(t, tx),
                    None => self.vim.reset(),
                }
            }
            Pending::Replace => self.vim_replace_chars(c, tx),
            Pending::SetMark => {
                self.vim.marks.insert(c, self.edit.focus);
                self.vim.reset();
            }
            Pending::Jump { line } => match self.vim.marks.get(&c).copied() {
                Some(at) => {
                    let text = &self.edit.text;
                    let mut at = at.min(text.len());
                    while !text.is_char_boundary(at) {
                        at -= 1;
                    }
                    let t = if line { motion::lines(motion::first_nonblank(text, at)) } else { Target { to: at, inclusive: false, linewise: false } };
                    self.vim_carry_out(t, tx);
                }
                None => {
                    self.file.notice = Some(format!("No mark {c}"));
                    self.vim.reset();
                }
            },
            Pending::Object { inner } => {
                let Some(object) = motion::object(&self.edit.text, self.edit.focus, inner, c) else { return self.vim.reset() };
                if self.vim.visual() {
                    self.vim.visual_anchor = Some(object.range.start);
                    if object.linewise {
                        self.vim.mode = Mode::VisualLine;
                    }
                    let last = motion::prev_char(&self.edit.text, object.range.end);
                    self.vim.reset();
                    self.vim_jump(last.max(object.range.start), tx);
                } else if let Some(op) = self.vim.op.take() {
                    self.vim_apply(op, object.range.start, object.range.end, object.linewise, tx);
                } else {
                    self.vim.reset();
                }
            }
            Pending::Register => self.vim.register = Some(c),
            Pending::MacroStore => {
                if c.is_ascii_alphanumeric() {
                    self.vim.macro_reg = Some(c);
                    self.vim.macro_buf.clear();
                }
                self.vim.reset();
            }
            Pending::MacroPlay => {
                let n = self.vim.total();
                self.vim.reset();
                let reg = if c == '@' { self.vim.last_macro } else { Some(c) };
                if let Some(keys) = reg.and_then(|r| self.vim.macros.get(&r).cloned()) {
                    self.vim.last_macro = reg;
                    self.vim_play(keys, n);
                }
            }
        }
    }

    fn vim_char(&mut self, c: char, tx: &mut Tx) {
        let pending = std::mem::take(&mut self.vim.pending);
        if pending != Pending::None {
            return self.vim_pending(pending, c, tx);
        }
        if c == '"' {
            self.vim.pending = Pending::Register;
            return;
        }
        if c.is_ascii_digit() && (c != '0' || self.vim.count > 0) {
            self.vim.count = self.vim.count.saturating_mul(10).saturating_add(c as u32 - '0' as u32).min(99_999);
            return;
        }
        match c {
            'g' => {
                self.vim.pending = Pending::G;
                return;
            }
            'f' | 'F' | 't' | 'T' => {
                self.vim.pending = Pending::Find(c);
                return;
            }
            '`' => {
                self.vim.pending = Pending::Jump { line: false };
                return;
            }
            '\'' => {
                self.vim.pending = Pending::Jump { line: true };
                return;
            }
            '/' | '?' => return self.vim_open_cmdline(c),
            _ => {}
        }
        if self.vim.visual() && self.vim_visual_key(c, tx) {
            return;
        }
        if self.vim.op.is_some() {
            match (self.vim.op, c) {
                (Some(Op::Lower), 'u') | (Some(Op::Upper), 'U') | (Some(Op::Toggle), '~') => return self.vim_line_op(self.vim.op.expect("matched"), tx),
                (_, 'i') => {
                    self.vim.pending = Pending::Object { inner: true };
                    return;
                }
                (_, 'a') => {
                    self.vim.pending = Pending::Object { inner: false };
                    return;
                }
                _ => {}
            }
        }
        let op = match c {
            'd' => Some(Op::Delete),
            'c' => Some(Op::Change),
            'y' => Some(Op::Yank),
            '>' => Some(Op::Indent),
            '<' => Some(Op::Outdent),
            _ => None,
        };
        if let Some(op) = op {
            return self.vim_op_key(op, tx);
        }
        if self.vim_motion(c, tx) {
            return;
        }
        if self.vim.op.is_some() || self.vim.visual() {
            return self.vim.reset();
        }
        let at = self.edit.focus;
        match c {
            'x' => {
                self.vim.op = Some(Op::Delete);
                self.vim_motion('l', tx);
            }
            'X' => {
                self.vim.op = Some(Op::Delete);
                self.vim_motion('h', tx);
            }
            's' => {
                self.vim.op = Some(Op::Change);
                self.vim_motion('l', tx);
            }
            'S' => self.vim_line_op(Op::Change, tx),
            'D' => {
                self.vim.op = Some(Op::Delete);
                self.vim_motion('$', tx);
            }
            'C' => {
                self.vim.op = Some(Op::Change);
                self.vim_motion('$', tx);
            }
            'Y' => self.vim_line_op(Op::Yank, tx),
            'r' => self.vim.pending = Pending::Replace,
            'R' => self.vim_enter_insert(Mode::Replace),
            '~' => self.vim_toggle_chars(tx),
            'J' => {
                let n = self.vim.total();
                self.vim_join(n.saturating_sub(1).max(1), tx);
            }
            'p' => self.vim_paste(true, tx),
            'P' => self.vim_paste(false, tx),
            'u' => self.vim_undo(false),
            '.' => {
                let n = self.vim.total();
                self.vim.reset();
                let keys = self.vim.last_change.clone();
                self.vim_play(keys, n);
            }
            'i' => self.vim_enter_insert(Mode::Insert),
            'a' => {
                if !self.edit.text[at..].starts_with('\n') {
                    let to = motion::next_char(&self.edit.text, at);
                    self.vim_caret(to);
                }
                self.vim_enter_insert(Mode::Insert);
            }
            'I' => {
                let to = motion::first_nonblank(&self.edit.text, at);
                self.vim_caret(to);
                self.vim_enter_insert(Mode::Insert);
            }
            'A' => {
                let to = motion::line_end(&self.edit.text, at);
                self.vim_caret(to);
                self.vim_enter_insert(Mode::Insert);
            }
            'o' => self.vim_open_line(true),
            'O' => self.vim_open_line(false),
            'v' | 'V' => {
                self.vim.visual_anchor = Some(at);
                self.vim.mode = if c == 'v' { Mode::Visual } else { Mode::VisualLine };
                self.vim.reset();
            }
            'q' => match self.vim.macro_reg.take() {
                Some(reg) => {
                    self.vim.macro_buf.pop();
                    let keys = std::mem::take(&mut self.vim.macro_buf);
                    self.vim.macros.insert(reg, keys);
                    self.vim.reset();
                }
                None => self.vim.pending = Pending::MacroStore,
            },
            '@' => self.vim.pending = Pending::MacroPlay,
            'm' => self.vim.pending = Pending::SetMark,
            ':' => self.vim_open_cmdline(':'),
            '*' => self.vim_search_word(true, tx),
            '#' => self.vim_search_word(false, tx),
            _ => self.vim.reset(),
        }
    }

    /// Control chords that mean something in Vim. Anything else goes on as usual.
    fn vim_control(&mut self, c: char, tx: &mut Tx) -> bool {
        let n = self.vim.total();
        match c {
            'r' => self.vim_undo(true),
            '[' => self.vim_escape(),
            'd' | 'u' | 'f' | 'b' => {
                self.vim.reset();
                let (key, lines) = match c {
                    'd' => (Key::Down, 15),
                    'u' => (Key::Up, 15),
                    'f' => (Key::Down, 30),
                    _ => (Key::Up, 30),
                };
                self.vim_arrows(key, lines * n, tx);
            }
            _ => return false,
        }
        true
    }
}

fn swap_find(kind: char) -> char {
    match kind {
        'f' => 'F',
        'F' => 'f',
        't' => 'T',
        _ => 't',
    }
}

fn single_char(t: &str) -> Option<char> {
    let mut chars = t.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

fn step(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if matches!(ev.key, Key::Modifier { .. }) {
        return false;
    }
    s.vim.record(ev);
    if s.vim.cmdline.is_some() {
        s.vim_cmdline_key(ev, tx);
        return true;
    }
    let inserting = matches!(s.vim.mode, Mode::Insert | Mode::Replace);
    if ev.mods.cmd || ev.mods.alt {
        // A command chord acts on what's selected, so Visual mode hands its selection over.
        if let Some(anchor) = s.vim.visual_anchor.take() {
            let (a, b, _) = visual_span(&s.edit.text, anchor, s.edit.focus, s.vim.mode == Mode::VisualLine);
            s.edit.set_selection(a, b);
            s.vim.mode = Mode::Normal;
        }
        s.vim.reset();
        return false;
    }
    if ev.mods.ctrl {
        let Key::Text(t) = &ev.key else { return false };
        let Some(c) = single_char(t) else { return false };
        if inserting {
            if c == '[' {
                s.vim_leave_insert();
                return true;
            }
            return false;
        }
        return s.vim_control(c, tx);
    }
    if inserting {
        if ev.key == Key::Escape {
            s.vim_leave_insert();
            return true;
        }
        if s.vim.mode == Mode::Replace
            && matches!(ev.key, Key::Text(_))
            && let Some(c) = s.edit.text[s.edit.focus..].chars().next().filter(|c| *c != '\n')
        {
            let at = s.edit.focus;
            s.edit.set_selection(at, at + c.len_utf8());
        }
        return false;
    }
    match &ev.key {
        Key::Escape => s.vim_escape(),
        Key::Text(t) => {
            if let Some(h) = &mut s.hover {
                h.armed = false;
            }
            if let Some(c) = single_char(t) {
                s.vim_char(c, tx);
            }
        }
        Key::Enter => {
            s.vim_motion('+', tx);
        }
        Key::Backspace => {
            s.vim_motion('h', tx);
        }
        Key::Delete => s.vim_char('x', tx),
        Key::Tab => {}
        _ => return false,
    }
    true
}

/// True when Vim took the key. ⌘ chords always go on, so every shortcut works in every mode.
pub fn handle_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if !s.vim.on {
        return false;
    }
    if s.vim.synthetic > 0 && is_arrow(ev) {
        s.vim.synthetic -= 1;
        return false;
    }
    if !matches!(ev.key, Key::Modifier { .. })
        && let Some(expected) = s.vim.expected.take()
        && expected != *ev
    {
        // Typed while a repeat runs. Escape stops the repeat, and anything else would land in the
        // middle of it, so it's dropped.
        if ev.key == Key::Escape {
            s.vim.queue.clear();
            s.vim.replaying = false;
            s.vim_escape();
        } else {
            s.vim.expected = Some(expected);
        }
        return true;
    }
    let took = step(s, ev, tx);
    s.vim.finish();
    match s.vim.queue.pop_front() {
        Some(next) => {
            s.vim.expected = Some(next.clone());
            tx.send_key(next);
            tx.wake_after(Duration::ZERO);
        }
        None => s.vim.replaying = false,
    }
    took
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_is_remembered_but_each_launch_starts_in_insert() {
        let dir = std::env::temp_dir().join(format!("alternate-vim-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut v = Vim::load(&dir);
        assert!(!v.on);
        v.toggle();
        v.mode = Mode::Normal;
        let back = Vim::load(&dir);
        assert!(back.on);
        assert_eq!(back.mode, Mode::Insert);
        v.toggle();
        assert!(!Vim::load(&dir).on);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn marks_follow_the_text() {
        let mut v = Vim::default();
        v.marks.insert('a', 10);
        v.marks.insert('b', 2);
        v.rebase_marks(Edit { start: 5, removed: 0, inserted: 3 });
        assert_eq!(v.marks[&'a'], 13);
        assert_eq!(v.marks[&'b'], 2);
        v.rebase_marks(Edit { start: 1, removed: 4, inserted: 0 });
        assert_eq!(v.marks[&'b'], 1);
    }
}
