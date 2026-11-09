//! One way of asking a model for text, whichever route reaches it. A request runs on its own
//! thread and its events are collected from `poll`, so the window never waits for the network.

pub mod alternatives;
pub mod anthropic;
pub mod claude_cli;
pub mod secrets;
pub mod settings;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Fast,
    Deep,
}

impl Tier {
    pub fn model(self) -> &'static str {
        match self {
            Tier::Fast => "claude-haiku-5-5",
            Tier::Deep => "claude-sonnet-5-5",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Format {
    /// Plain text, delivered one finished line at a time.
    Lines,
    /// One JSON value matching this schema.
    Schema(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiRequest {
    pub tier: Tier,
    pub system: String,
    pub user: String,
    pub format: Format,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiError {
    /// The login, token or key was refused or has run out.
    Credentials,
    /// Nothing saved yet for the chosen route.
    NoSecret,
    /// The claude command turned the sign-in down, and why.
    SignIn(String),
    NoNetwork,
    RateLimited,
    MissingCli,
    Refused,
    Other(String),
}

impl AiError {
    pub fn message(&self) -> String {
        match self {
            AiError::Credentials => "Your Claude sign-in has expired or was refused. Update it in AI settings.".to_string(),
            AiError::NoSecret => "Nothing is saved for this sign-in yet. Add it in AI settings.".to_string(),
            AiError::SignIn(why) => format!("Claude refused the sign-in: {why} Update it in AI settings."),
            AiError::NoNetwork => "Couldn't reach Claude. Check your connection and try again.".to_string(),
            AiError::RateLimited => "You've hit a usage limit. Try again in a little while.".to_string(),
            AiError::MissingCli => "Couldn't find the claude command. Install Claude Code, or choose an API key in AI settings.".to_string(),
            AiError::Refused => "Claude declined to work on this text.".to_string(),
            AiError::Other(why) => why.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AiEvent {
    Line(String),
    Json(Value),
    Done,
    Failed(AiError),
}

pub trait Provider: Send + Sync {
    /// Runs one request to its end. Must send exactly one `Done` or `Failed`, last.
    fn run(&self, req: &AiRequest, sink: &mut dyn FnMut(AiEvent), cancel: &AtomicBool);
}

/// A request in flight. Dropping it cancels it.
pub struct Job {
    rx: Receiver<AiEvent>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn start(provider: Arc<dyn Provider>, req: AiRequest) -> Job {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::Builder::new()
            .name("alternate-ai".into())
            .spawn(move || provider.run(&req, &mut |e| drop(tx.send(e)), &flag))
            .expect("the OS gives us a thread");
        Job { rx, cancel }
    }

    pub fn drain(&self) -> Vec<AiEvent> {
        self.rx.try_iter().collect()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Turns streamed fragments into finished lines. Blank lines are dropped.
#[derive(Default)]
pub struct LineBuffer {
    partial: String,
}

impl LineBuffer {
    pub fn push(&mut self, delta: &str, sink: &mut dyn FnMut(AiEvent)) {
        self.partial.push_str(delta);
        while let Some(at) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=at).collect();
            emit(&line, sink);
        }
    }

    pub fn finish(&mut self, sink: &mut dyn FnMut(AiEvent)) {
        emit(&std::mem::take(&mut self.partial), sink);
    }
}

fn emit(line: &str, sink: &mut dyn FnMut(AiEvent)) {
    let line = line.trim();
    if !line.is_empty() {
        sink(AiEvent::Line(line.to_string()));
    }
}

/// Runs a request and hands back its lines, for `--ai-check`.
pub fn check(provider: Arc<dyn Provider>, text: &str) -> Result<Vec<String>, AiError> {
    let req = AiRequest { tier: Tier::Fast, system: "Answer briefly.".to_string(), user: text.to_string(), format: Format::Lines };
    let cancel = AtomicBool::new(false);
    let (mut lines, mut out) = (Vec::new(), None);
    provider.run(&req, &mut |e| match e {
        AiEvent::Line(l) => lines.push(l),
        AiEvent::Failed(err) => out = Some(err),
        _ => {}
    }, &cancel);
    out.map_or(Ok(lines), Err)
}

/// A provider whose events a test feeds in by hand.
#[cfg(test)]
pub fn fake() -> (Arc<dyn Provider>, std::sync::mpsc::Sender<AiEvent>) {
    use std::sync::Mutex;
    struct Fake(Mutex<Receiver<AiEvent>>);
    impl Provider for Fake {
        fn run(&self, _: &AiRequest, sink: &mut dyn FnMut(AiEvent), cancel: &AtomicBool) {
            let rx = self.0.lock().unwrap();
            while !cancel.load(Ordering::Relaxed) {
                if let Ok(e) = rx.recv_timeout(std::time::Duration::from_millis(5)) {
                    let last = matches!(e, AiEvent::Done | AiEvent::Failed(_));
                    sink(e);
                    if last {
                        return;
                    }
                }
            }
        }
    }
    let (tx, rx) = channel();
    (Arc::new(Fake(Mutex::new(rx))), tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(parts: &[&str]) -> Vec<String> {
        let mut b = LineBuffer::default();
        let mut out = Vec::new();
        let mut sink = |e| if let AiEvent::Line(l) = e { out.push(l) };
        for p in parts {
            b.push(p, &mut sink);
        }
        b.finish(&mut sink);
        out
    }

    #[test]
    fn fragments_become_whole_lines() {
        assert_eq!(lines_of(&["one\ntw", "o\n\nthr", "ee"]), ["one", "two", "three"]);
    }

    #[test]
    fn no_error_message_is_empty() {
        for e in [AiError::Credentials, AiError::NoNetwork, AiError::RateLimited, AiError::MissingCli, AiError::Refused] {
            assert!(!e.message().is_empty());
        }
    }

    #[test]
    fn events_arrive_from_another_thread_and_dropping_cancels() {
        struct Slow;
        impl Provider for Slow {
            fn run(&self, _: &AiRequest, sink: &mut dyn FnMut(AiEvent), cancel: &AtomicBool) {
                sink(AiEvent::Line("first".to_string()));
                while !cancel.load(Ordering::Relaxed) {
                    std::thread::yield_now();
                }
            }
        }
        let req = AiRequest { tier: Tier::Fast, system: String::new(), user: String::new(), format: Format::Lines };
        let job = Job::start(Arc::new(Slow), req);
        let mut got = Vec::new();
        while got.is_empty() {
            got = job.drain();
        }
        assert_eq!(got, [AiEvent::Line("first".to_string())]);
        drop(job);
    }
}
