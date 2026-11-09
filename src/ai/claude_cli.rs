//! The subscription route: the local `claude` command, in a bare environment. A subscription token
//! is only ever given to that command, never sent anywhere by this app.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use serde_json::Value;

use super::{AiError, AiEvent, AiRequest, Format, LineBuffer};

/// Where the command usually is. An app opened from Finder doesn't get the shell's PATH.
fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".local/bin/claude"), home.join(".claude/local/claude"), PathBuf::from("/opt/homebrew/bin/claude"), PathBuf::from("/usr/local/bin/claude")]
}

/// The stored path if it still exists, otherwise the first usual place that has the command.
pub fn find(stored: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = stored.filter(|p| p.is_file()) {
        return Some(p.to_path_buf());
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    candidates(&home).into_iter().find(|p| p.is_file())
}

pub fn args(req: &AiRequest) -> Vec<String> {
    let mut out: Vec<String> = ["-p", "--model", req.tier.model(), "--tools", "", "--strict-mcp-config", "--setting-sources", "", "--no-session-persistence", "--disable-slash-commands", "--system-prompt", &req.system].iter().map(|s| s.to_string()).collect();
    match &req.format {
        Format::Lines => out.extend(["--output-format", "stream-json", "--verbose", "--include-partial-messages"].map(String::from)),
        Format::Schema(schema) => out.extend(["--output-format".to_string(), "json".to_string(), "--json-schema".to_string(), schema.to_string()]),
    }
    out
}

/// Everything the command is allowed to see. An API key in the app's own environment would
/// outrank the token, so nothing is inherited.
pub fn environment(claude: &Path, home: &str, token: Option<&str>) -> Vec<(String, String)> {
    let dir = claude.parent().map_or(String::new(), |d| format!("{}:", d.display()));
    let mut env = vec![("HOME".to_string(), home.to_string()), ("PATH".to_string(), format!("{dir}/usr/bin:/bin"))];
    if let Some(token) = token {
        env.push(("CLAUDE_CODE_OAUTH_TOKEN".to_string(), token.to_string()));
    }
    env
}

pub fn classify(text: &str) -> AiError {
    let t = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["authenticate", "log in", "login", "expired", "unauthorized", "invalid api key", "oauth", "401"]) {
        AiError::Credentials
    } else if has(&["rate limit", "usage limit", "limit reached", "429", "overloaded"]) {
        AiError::RateLimited
    } else if has(&["econnrefused", "enotfound", "network", "connection", "timed out", "fetch failed"]) {
        AiError::NoNetwork
    } else {
        AiError::Other("The claude command couldn't finish this. Try again.".to_string())
    }
}

/// Reads what the command prints, one JSON document per line.
pub struct CliParser {
    schema: bool,
    lines: LineBuffer,
    finished: bool,
}

impl CliParser {
    pub fn new(format: &Format) -> CliParser {
        CliParser { schema: matches!(format, Format::Schema(_)), lines: LineBuffer::default(), finished: false }
    }

    pub fn feed(&mut self, line: &str, sink: &mut dyn FnMut(AiEvent)) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        if self.finished {
            return;
        }
        match v["type"].as_str() {
            Some("stream_event") if v["event"]["type"] == "content_block_delta" && v["event"]["delta"]["type"] == "text_delta" => {
                self.lines.push(v["event"]["delta"]["text"].as_str().unwrap_or(""), sink);
            }
            Some("result") => self.result(&v, sink),
            _ => {}
        }
    }

    fn result(&mut self, v: &Value, sink: &mut dyn FnMut(AiEvent)) {
        self.finished = true;
        if v["is_error"].as_bool().unwrap_or(false) {
            let text = v["result"].as_str().or(v["subtype"].as_str()).unwrap_or("");
            return sink(AiEvent::Failed(classify(text)));
        }
        if self.schema {
            match v.get("structured_output").filter(|s| !s.is_null()) {
                Some(out) => sink(AiEvent::Json(out.clone())),
                None => return sink(AiEvent::Failed(AiError::Other("Claude's answer came back unreadable. Try again.".to_string()))),
            }
        } else {
            self.lines.finish(sink);
        }
        sink(AiEvent::Done);
    }

    /// The command exited. If it never gave a result, say why as well as its error output allows.
    pub fn end(&mut self, stderr: &str, sink: &mut dyn FnMut(AiEvent)) {
        if !self.finished {
            self.finished = true;
            sink(AiEvent::Failed(classify(stderr)));
        }
    }
}

pub fn run(claude: &Path, token: Option<&str>, req: &AiRequest, sink: &mut dyn FnMut(AiEvent), cancel: &AtomicBool) {
    let home = std::env::var("HOME").unwrap_or_default();
    let cwd = std::env::temp_dir().join("alternate-claude-cwd");
    if std::fs::create_dir_all(&cwd).is_err() {
        return sink(AiEvent::Failed(AiError::Other("Couldn't make a working folder for the claude command.".to_string())));
    }
    let mut command = Command::new(claude);
    command.args(args(req)).env_clear().envs(environment(claude, &home, token)).current_dir(&cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(_) => return sink(AiEvent::Failed(AiError::MissingCli)),
    };
    let (Some(mut stdin), Some(stdout), Some(mut stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return sink(AiEvent::Failed(AiError::MissingCli));
    };
    let user = req.user.clone();
    std::thread::spawn(move || drop(stdin.write_all(user.as_bytes())));
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut parser = CliParser::new(&req.format);
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return sink(AiEvent::Done);
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => parser.feed(&line, sink),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.wait();
    parser.end(&errors.join().unwrap_or_default(), sink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::Tier;
    use serde_json::json;

    /// What the real command printed for an expired login.
    const EXPIRED: &str = "Failed to authenticate: this login reached its time limit and cannot be renewed. Run `claude auth login` to sign in again, or use `claude setup-token` and set CLAUDE_CODE_OAUTH_TOKEN for unattended jobs";

    fn delta(t: &str) -> String {
        json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":t}}}).to_string()
    }

    fn parse(format: Format, lines: &[String], stderr: &str) -> Vec<AiEvent> {
        let mut p = CliParser::new(&format);
        let mut out = Vec::new();
        for l in lines {
            p.feed(l, &mut |e| out.push(e));
        }
        p.end(stderr, &mut |e| out.push(e));
        out
    }

    fn request(format: Format) -> AiRequest {
        AiRequest { tier: Tier::Fast, system: "sys".into(), user: "u".into(), format }
    }

    #[test]
    fn partial_messages_become_lines_then_done() {
        let result = json!({"type":"result","subtype":"success","is_error":false,"result":"one\ntwo"}).to_string();
        let init = json!({"type":"system","subtype":"init"}).to_string();
        let got = parse(Format::Lines, &[init, delta("one\nt"), delta("wo"), result], "");
        assert_eq!(got, [AiEvent::Line("one".into()), AiEvent::Line("two".into()), AiEvent::Done]);
    }

    #[test]
    fn schema_results_come_from_structured_output() {
        let result = json!({"type":"result","is_error":false,"result":"","structured_output":{"a":1}}).to_string();
        assert_eq!(parse(Format::Schema(json!({})), &[result], ""), [AiEvent::Json(json!({"a":1})), AiEvent::Done]);
    }

    #[test]
    fn a_missing_structured_output_is_a_failure() {
        let result = json!({"type":"result","is_error":false,"result":"text"}).to_string();
        assert!(matches!(parse(Format::Schema(json!({})), &[result], "")[0], AiEvent::Failed(AiError::Other(_))));
    }

    #[test]
    fn the_real_expired_login_message_means_credentials() {
        let result = json!({"type":"result","subtype":"success","is_error":true,"result":EXPIRED}).to_string();
        assert_eq!(parse(Format::Lines, &[result], ""), [AiEvent::Failed(AiError::Credentials)]);
    }

    #[test]
    fn exiting_without_a_result_uses_the_error_output() {
        assert_eq!(parse(Format::Lines, &[delta("par")], "API Error: 429 rate limit"), [AiEvent::Failed(AiError::RateLimited)]);
    }

    #[test]
    fn malformed_output_is_skipped() {
        let result = json!({"type":"result","is_error":false,"result":"ok"}).to_string();
        assert_eq!(parse(Format::Lines, &["{cut".into(), delta("ok"), result], ""), [AiEvent::Line("ok".into()), AiEvent::Done]);
    }

    #[test]
    fn nothing_from_the_users_setup_is_loaded() {
        let a = args(&request(Format::Lines));
        let after = |flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].clone();
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--setting-sources"), "");
        assert!(a.contains(&"--strict-mcp-config".to_string()));
        assert!(a.contains(&"--no-session-persistence".to_string()));
        assert!(!a.contains(&"--bare".to_string()), "bare mode ignores the token");
    }

    #[test]
    fn lines_stream_and_schemas_do_not() {
        assert!(args(&request(Format::Lines)).contains(&"stream-json".to_string()));
        let a = args(&request(Format::Schema(json!({"type":"object"}))));
        assert!(!a.contains(&"stream-json".to_string()));
        assert!(a.contains(&r#"{"type":"object"}"#.to_string()));
    }

    #[test]
    fn the_prompt_text_is_not_on_the_command_line() {
        assert!(!args(&request(Format::Lines)).contains(&"u".to_string()));
    }

    #[test]
    fn only_the_token_is_passed_and_never_an_api_key() {
        let env = environment(Path::new("/x/bin/claude"), "/home/me", Some("tok"));
        assert!(env.iter().all(|(k, _)| k != "ANTHROPIC_API_KEY"));
        assert!(env.contains(&("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "tok".to_string())));
        assert!(env.contains(&("PATH".to_string(), "/x/bin:/usr/bin:/bin".to_string())));
        assert_eq!(environment(Path::new("/x/claude"), "/h", None).len(), 2);
    }

    #[test]
    fn a_stored_path_that_vanished_is_not_used() {
        let gone = PathBuf::from("/nonexistent/claude");
        assert_ne!(find(Some(&gone)), Some(gone));
    }

    #[test]
    fn a_command_that_cannot_start_says_so() {
        let mut out = Vec::new();
        run(Path::new("/nonexistent/claude"), None, &request(Format::Lines), &mut |e| out.push(e), &AtomicBool::new(false));
        assert_eq!(out, [AiEvent::Failed(AiError::MissingCli)]);
    }
}
