//! The direct route: an API key and the Messages API over HTTP.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::{AiError, AiEvent, AiRequest, Format, LineBuffer, Tier};

const URL: &str = "https://api.anthropic.com/v1/messages";
const VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 64_000;

pub fn body(req: &AiRequest) -> Value {
    let mut output = json!({ "effort": if req.tier == Tier::Fast { "low" } else { "medium" } });
    if let Format::Schema(schema) = &req.format {
        output["format"] = json!({ "type": "json_schema", "schema": schema });
    }
    let mut body = json!({
        "model": req.tier.model(),
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": req.system,
        "messages": [{ "role": "user", "content": req.user }],
        "output_config": output,
    });
    if req.tier == Tier::Deep {
        body["fallbacks"] = json!("default");
    }
    body
}

/// Reads the event stream line by line.
pub struct SseParser {
    schema: bool,
    lines: LineBuffer,
    text: String,
    stop: Option<String>,
    finished: bool,
}

impl SseParser {
    pub fn new(format: &Format) -> SseParser {
        SseParser { schema: matches!(format, Format::Schema(_)), lines: LineBuffer::default(), text: String::new(), stop: None, finished: false }
    }

    pub fn feed(&mut self, line: &str, sink: &mut dyn FnMut(AiEvent)) {
        let Some(data) = line.strip_prefix("data:") else { return };
        let Ok(v) = serde_json::from_str::<Value>(data.trim()) else { return };
        if self.finished {
            return;
        }
        match v["type"].as_str() {
            Some("content_block_delta") if v["delta"]["type"] == "text_delta" => {
                let delta = v["delta"]["text"].as_str().unwrap_or("");
                if self.schema {
                    self.text.push_str(delta);
                } else {
                    self.lines.push(delta, sink);
                }
            }
            Some("message_delta") => self.stop = v["delta"]["stop_reason"].as_str().map(str::to_string),
            Some("message_stop") => self.complete(sink),
            Some("error") => {
                self.finished = true;
                sink(AiEvent::Failed(api_error(v["error"]["type"].as_str().unwrap_or(""), v["error"]["message"].as_str().unwrap_or("The request failed."))));
            }
            _ => {}
        }
    }

    fn complete(&mut self, sink: &mut dyn FnMut(AiEvent)) {
        self.finished = true;
        if self.stop.as_deref() == Some("refusal") {
            return sink(AiEvent::Failed(AiError::Refused));
        }
        if self.schema {
            match serde_json::from_str(&self.text) {
                Ok(v) => sink(AiEvent::Json(v)),
                Err(_) => return sink(AiEvent::Failed(AiError::Other("Claude's answer came back unreadable. Try again.".to_string()))),
            }
        } else {
            self.lines.finish(sink);
        }
        sink(AiEvent::Done);
    }

    /// The stream ended. If it never said it was finished, it was cut off.
    pub fn end(&mut self, sink: &mut dyn FnMut(AiEvent)) {
        if !self.finished {
            self.finished = true;
            sink(AiEvent::Failed(AiError::NoNetwork));
        }
    }
}

fn api_error(kind: &str, message: &str) -> AiError {
    match kind {
        "authentication_error" | "permission_error" => AiError::Credentials,
        "rate_limit_error" | "overloaded_error" => AiError::RateLimited,
        _ => AiError::Other(message.to_string()),
    }
}

pub fn status_error(code: u16, body: &str) -> AiError {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let (kind, message) = (parsed["error"]["type"].as_str().unwrap_or(""), parsed["error"]["message"].as_str().unwrap_or(""));
    match code {
        401 | 403 => AiError::Credentials,
        429 | 529 => AiError::RateLimited,
        _ if !message.is_empty() => api_error(kind, message),
        _ => AiError::Other(format!("Claude returned an error ({code}). Try again.")),
    }
}

pub fn run(key: &str, req: &AiRequest, sink: &mut dyn FnMut(AiEvent), cancel: &AtomicBool) {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(180)).build();
    let mut call = agent.post(URL).set("content-type", "application/json").set("x-api-key", key).set("anthropic-version", VERSION);
    if req.tier == Tier::Deep {
        call = call.set("anthropic-beta", FALLBACK_BETA);
    }
    let response = match call.send_string(&body(req).to_string()) {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => return sink(AiEvent::Failed(status_error(code, &r.into_string().unwrap_or_default()))),
        Err(ureq::Error::Transport(_)) => return sink(AiEvent::Failed(AiError::NoNetwork)),
    };
    let mut parser = SseParser::new(&req.format);
    for line in BufReader::new(response.into_reader()).lines() {
        if cancel.load(Ordering::Relaxed) {
            return sink(AiEvent::Done);
        }
        match line {
            Ok(l) => parser.feed(&l, sink),
            Err(_) => break,
        }
    }
    parser.end(sink);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(t: &str) -> String {
        format!("data: {}", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":t}}))
    }

    const STOP: &str = r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#;
    const END: &str = r#"data: {"type":"message_stop"}"#;

    fn parse(format: Format, lines: &[String]) -> Vec<AiEvent> {
        let mut p = SseParser::new(&format);
        let mut out = Vec::new();
        for l in lines {
            p.feed(l, &mut |e| out.push(e));
        }
        p.end(&mut |e| out.push(e));
        out
    }

    #[test]
    fn a_stream_of_text_becomes_lines_then_done() {
        let got = parse(Format::Lines, &["event: message_start".into(), delta("one\ntw"), delta("o"), STOP.into(), END.into()]);
        assert_eq!(got, [AiEvent::Line("one".into()), AiEvent::Line("two".into()), AiEvent::Done]);
    }

    #[test]
    fn schema_output_arrives_as_one_value() {
        let got = parse(Format::Schema(json!({})), &[delta("{\"a\""), delta(":1}"), STOP.into(), END.into()]);
        assert_eq!(got, [AiEvent::Json(json!({"a":1})), AiEvent::Done]);
    }

    #[test]
    fn a_stream_cut_short_is_a_failure() {
        let got = parse(Format::Lines, &[delta("one\n")]);
        assert_eq!(got, [AiEvent::Line("one".into()), AiEvent::Failed(AiError::NoNetwork)]);
    }

    #[test]
    fn garbage_lines_are_skipped() {
        let got = parse(Format::Lines, &["data: {not json".into(), ": ping".into(), delta("ok"), END.into()]);
        assert_eq!(got, [AiEvent::Line("ok".into()), AiEvent::Done]);
    }

    #[test]
    fn broken_schema_output_is_a_failure() {
        let got = parse(Format::Schema(json!({})), &[delta("{\"a\""), END.into()]);
        assert!(matches!(got[0], AiEvent::Failed(AiError::Other(_))));
    }

    #[test]
    fn a_refusal_is_reported() {
        let stop = r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal"}}"#;
        assert_eq!(parse(Format::Lines, &[stop.into(), END.into()]), [AiEvent::Failed(AiError::Refused)]);
    }

    #[test]
    fn a_mid_stream_error_is_reported_once() {
        let err = r#"data: {"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
        assert_eq!(parse(Format::Lines, &[err.into()]), [AiEvent::Failed(AiError::RateLimited)]);
    }

    #[test]
    fn statuses_map_to_what_the_person_can_do() {
        assert_eq!(status_error(401, ""), AiError::Credentials);
        assert_eq!(status_error(429, ""), AiError::RateLimited);
        assert_eq!(status_error(400, r#"{"error":{"type":"invalid_request_error","message":"bad"}}"#), AiError::Other("bad".into()));
    }

    fn request(tier: Tier, format: Format) -> AiRequest {
        AiRequest { tier, system: "s".into(), user: "u".into(), format }
    }

    #[test]
    fn the_body_names_the_tier_and_asks_for_a_schema_only_when_wanted() {
        let fast = body(&request(Tier::Fast, Format::Lines));
        assert_eq!(fast["model"], "claude-haiku-5-5");
        assert_eq!(fast["output_config"]["format"], Value::Null);
        assert_eq!(fast["fallbacks"], Value::Null);
        let deep = body(&request(Tier::Deep, Format::Schema(json!({"type":"object"}))));
        assert_eq!(deep["model"], "claude-sonnet-5-5");
        assert_eq!(deep["output_config"]["format"]["schema"]["type"], "object");
        assert_eq!(deep["fallbacks"], "default");
    }
}
