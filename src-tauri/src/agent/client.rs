//! The blocking HTTP client for ai-desktop: `GET /health` and `POST /ask`.
//!
//! **Why Rust makes this call.** ai-desktop rejects any request that carries a
//! foreign `Origin` header (`src/main.cpp`, `is_allowed_origin`), and a browser
//! always sends one, so the webview cannot reach it at all. This client sends
//! no `Origin`, which is what a non-browser caller does and what passes.
//!
//! **Why loopback only.** ai-desktop binds `127.0.0.1`. The base address is a
//! setting (the port is configurable there), but it must name this machine:
//! the request holds file excerpts of a repository, and a setting that could
//! point at another host would route them there without D6's "where does this
//! go" question ever being asked. Anything else is refused when it is set.
//! Proxies from the environment are switched off for the same reason: a
//! `HTTP_PROXY` must not catch a call to localhost.
//!
//! **What an answer looks like.** `/ask` answers `{text, provider, stop_reason,
//! usage}`; a failure is `{"error": {"message": ...}}` with 400 (bad request),
//! 403 (foreign origin) or 502 (the backend failed: down, no key, its own
//! timeout). Each is a distinct [`AskError`] that names its cause, because
//! "request failed" is the sentence a person cannot act on.
//!
//! **Cancel.** A blocking read cannot be interrupted, so the request runs on a
//! helper thread and the caller waits on a channel, looking at its [`Stop`]
//! flag every 40 ms. On a stop the caller returns at once and the helper is
//! abandoned: it ends by itself when the request's own timeout passes, and its
//! answer is dropped. ai-desktop may still finish the call upstream; a closed
//! socket does not reach a cloud provider.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{Stop, StopKind};
use crate::apperr::AppError;

/// Where ai-desktop listens unless told otherwise (its `kDefaultPort`).
pub const DEFAULT_URL: &str = "http://127.0.0.1:8080";

/// How long `/health` may take: it answers from memory.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
/// ai-desktop enforces `timeout_ms` itself and answers 502 with the cause. This
/// is the margin before this side gives up as well, so the cause is the one
/// ai-desktop names and not a bare "no answer".
const SLACK: Duration = Duration::from_secs(15);
/// An answer bigger than this is not read.
const MAX_BODY: u64 = 16 * 1024 * 1024;
/// The longest piece of a foreign body quoted in a message.
const SNIPPET: usize = 200;

/// The four destinations ai-desktop routes to. The names are `/health`'s keys
/// and the values of `agent.backends` in `.rules.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Ollama,
    Openai,
    Anthropic,
    Gemini,
}

impl Backend {
    /// Where a model name is routed: `claude-` Anthropic, `gpt-`/`o1-`/`o3-`
    /// OpenAI, `gemini-` Google, anything else (no model included) Ollama.
    ///
    /// This is ai-desktop's rule for the default prefixes. Two things can move
    /// a model elsewhere there and are not known here: a custom model
    /// registered in its `config.json`, and `AI_DESKTOP_PREFIXES_*`. The
    /// destination this computes is the one shown and checked against the
    /// allow-list; computing it in one place is what keeps the dialog and the
    /// check from disagreeing.
    pub fn resolve(model: &str) -> Backend {
        let m = model.trim();
        if m.starts_with("claude-") {
            Backend::Anthropic
        } else if ["gpt-", "o1-", "o3-"].iter().any(|p| m.starts_with(p)) {
            Backend::Openai
        } else if m.starts_with("gemini-") {
            Backend::Gemini
        } else {
            Backend::Ollama
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Backend::Ollama => "ollama",
            Backend::Openai => "openai",
            Backend::Anthropic => "anthropic",
            Backend::Gemini => "gemini",
        }
    }

    /// Stays on this machine.
    pub fn is_local(self) -> bool {
        self == Backend::Ollama
    }
}

/// Where a run would send its requests, resolved once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub backend: Backend,
    pub model: String,
    pub local: bool,
}

/// The destination of `model`, or why the project does not allow it.
///
/// `allow` is `agent.backends` of `.rules.json` as `rules/plan` reports it, and
/// the rule is `rules.agent.destination`'s: a non-empty list is the whole
/// allow-list; an empty one means **local only** (D6: the default, so a cloud
/// backend is opt-in per project).
pub fn destination(model: &str, allow: &[String]) -> Result<Destination, AppError> {
    let model = model.trim();
    if model.is_empty() {
        return Err(AppError::new("err.agent.model"));
    }
    let backend = Backend::resolve(model);
    if allow.is_empty() {
        if !backend.is_local() {
            return Err(AppError::new("err.agent.localOnly")
                .with("backend", backend.name())
                .with("model", model));
        }
    } else if !allow
        .iter()
        .any(|b| b.trim().eq_ignore_ascii_case(backend.name()))
    {
        return Err(AppError::new("err.agent.notAllowed")
            .with("backend", backend.name())
            .with("model", model)
            .with("allowed", allow.join(", ")));
    }
    Ok(Destination {
        backend,
        model: model.to_string(),
        local: backend.is_local(),
    })
}

/// A base address that names ai-desktop on this machine, normalised to
/// `http://host:port`.
pub fn check_url(raw: &str) -> Result<String, AppError> {
    let bad = || AppError::new("err.agent.url").with("url", raw.trim());
    let rest = raw.trim().strip_prefix("http://").ok_or_else(bad)?;
    let rest = rest.trim_end_matches('/');
    // No path, query, credentials or whitespace.
    if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ', '\\']) {
        return Err(bad());
    }
    let (host, port) = if rest.starts_with('[') {
        // `[::1]` or `[::1]:port`
        let end = rest.find(']').ok_or_else(bad)?;
        let tail = &rest[end + 1..];
        let port = match tail {
            "" => None,
            t => Some(t.strip_prefix(':').ok_or_else(bad)?),
        };
        (&rest[..=end], port)
    } else {
        match rest.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (rest, None),
        }
    };
    let loopback = ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .any(|l| host.eq_ignore_ascii_case(l));
    if !loopback {
        return Err(bad());
    }
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(bad)?,
        None => 80,
    };
    Ok(format!("http://{host}:{port}"))
}

/// One `/ask` call.
#[derive(Debug, Clone)]
pub struct AskRequest {
    pub prompt: String,
    pub system: String,
    pub model: String,
    pub timeout_ms: u64,
}

/// What a backend reported using, from `/ask`'s `usage`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
    }
}

/// A successful `/ask`.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub text: String,
    pub provider: Option<String>,
    pub stop_reason: Option<String>,
    pub usage: Usage,
}

/// Why `/ask` (or `/health`) gave no answer, each with the cause in words.
#[derive(Debug, Clone, PartialEq)]
pub enum AskError {
    /// Nothing listens there, or the connection broke.
    Unreachable(String),
    /// 403: ai-desktop took the request for a cross-origin one.
    Forbidden(String),
    /// 4xx other than 403, or a redirect: ai-desktop did not accept the request.
    Rejected { status: u16, message: String },
    /// 5xx: ai-desktop is up and its backend failed (down, no key, its own timeout).
    Backend { status: u16, message: String },
    /// No answer within this many milliseconds.
    Timeout(u64),
    /// An answer that is not the shape `/ask` promises.
    Malformed(String),
    /// A stop was requested while the request was in flight.
    Stopped(StopKind),
}

impl AskError {
    /// The message with a key, naming `url` where the address is the point.
    pub fn into_app(self, url: &str) -> AppError {
        match self {
            AskError::Unreachable(e) => AppError::new("err.agent.unreachable")
                .with("url", url)
                .detail(e),
            AskError::Forbidden(m) => AppError::new("err.agent.origin").with("url", url).detail(m),
            AskError::Rejected { status, message } => AppError::new("err.agent.rejected")
                .with("status", status)
                .detail(message),
            AskError::Backend { status, message } => AppError::new("err.agent.backend")
                .with("status", status)
                .detail(message),
            AskError::Timeout(ms) => {
                AppError::new("err.agent.timeout").with("secs", ms.div_ceil(1000).max(1))
            }
            AskError::Malformed(m) => AppError::new("err.agent.malformed").detail(m),
            AskError::Stopped(_) => AppError::new("err.agent.stopped"),
        }
    }

    /// An error after which the next batch would fail the same way, so the run
    /// stops and keeps its queue instead of working through it.
    pub fn is_fatal(&self) -> bool {
        match self {
            // This request is too large: the next one may well fit.
            AskError::Rejected { status: 413, .. } => false,
            AskError::Unreachable(_) | AskError::Forbidden(_) | AskError::Rejected { .. } => true,
            _ => false,
        }
    }
}

/// `/health`, presence only: which backends ai-desktop reports as configured.
/// Only `true`/`false` values of the `backends` object are read, so nothing
/// else in the answer can end up here, a key included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub status: String,
    pub backends: BTreeMap<String, bool>,
}

/// A client for one ai-desktop address.
#[derive(Debug, Clone)]
pub struct Client {
    base: String,
    slack: Duration,
}

impl Client {
    /// `base` is checked by [`check_url`].
    pub fn new(base: &str) -> Result<Client, AppError> {
        Ok(Client {
            base: check_url(base)?,
            slack: SLACK,
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    #[cfg(test)]
    pub fn with_slack(mut self, slack: Duration) -> Client {
        self.slack = slack;
        self
    }

    fn agent(timeout: Duration) -> ureq::Agent {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .proxy(None)
                .max_redirects(0)
                .max_redirects_will_error(false)
                .timeout_global(Some(timeout))
                .build(),
        )
    }

    /// `GET /health`.
    pub fn health(&self) -> Result<Health, AskError> {
        let agent = Client::agent(HEALTH_TIMEOUT);
        let url = format!("{}/health", self.base);
        let mut resp = agent.get(&url).call().map_err(map_err(HEALTH_TIMEOUT))?;
        let status = resp.status().as_u16();
        let body = read_body(&mut resp, HEALTH_TIMEOUT.as_millis() as u64)?;
        let value = ok_json(status, &body)?;
        let mut backends = BTreeMap::new();
        if let Some(map) = value.get("backends").and_then(Value::as_object) {
            for (name, v) in map {
                if let Some(b) = v.as_bool() {
                    backends.insert(name.clone(), b);
                }
            }
        }
        Ok(Health {
            status: value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            backends,
        })
    }

    /// `POST /ask`, abandoned the moment `stop` is set.
    pub fn ask(&self, req: &AskRequest, stop: &Stop) -> Result<Reply, AskError> {
        // Nothing is sent once a stop is asked for.
        if let Some(kind) = stop.requested() {
            return Err(AskError::Stopped(kind));
        }
        let limit = Duration::from_millis(req.timeout_ms) + self.slack;
        let agent = Client::agent(limit);
        let url = format!("{}/ask", self.base);
        let body = json!({
            "prompt": req.prompt,
            "system": req.system,
            "model": req.model,
            "timeout_ms": req.timeout_ms,
        })
        .to_string();
        let timeout_ms = req.timeout_ms;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let out = (|| {
                let mut resp = agent
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .send(body)
                    .map_err(map_err_ms(timeout_ms))?;
                let status = resp.status().as_u16();
                let body = read_body(&mut resp, timeout_ms)?;
                Ok((status, body))
            })();
            // The receiver is gone when the caller stopped: nobody wants it.
            let _ = tx.send(out);
        });
        // A reply that is already there is taken even if a stop came in at the
        // same moment: it was paid for, and the caller keeps what returned.
        let (status, body) = loop {
            match rx.recv_timeout(Duration::from_millis(40)) {
                Ok(result) => break result?,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(kind) = stop.requested() {
                        return Err(AskError::Stopped(kind));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(AskError::Unreachable("the request ended abnormally".into()))
                }
            }
        };
        parse_reply(status, &body)
    }
}

fn map_err(limit: Duration) -> impl Fn(ureq::Error) -> AskError {
    map_err_ms(limit.as_millis() as u64)
}

fn map_err_ms(ms: u64) -> impl Fn(ureq::Error) -> AskError {
    move |e| match e {
        ureq::Error::Timeout(_) => AskError::Timeout(ms),
        ureq::Error::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            AskError::Timeout(ms)
        }
        ureq::Error::Protocol(_) | ureq::Error::Http(_) | ureq::Error::BodyExceedsLimit(_) => {
            AskError::Malformed(e.to_string())
        }
        other => AskError::Unreachable(other.to_string()),
    }
}

fn read_body(
    resp: &mut ureq::http::Response<ureq::Body>,
    limit_ms: u64,
) -> Result<String, AskError> {
    resp.body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(|e| match e {
            ureq::Error::Timeout(_) => AskError::Timeout(limit_ms),
            other => AskError::Malformed(format!("the answer could not be read: {other}")),
        })
}

/// `{"error": {"message": ...}}` of ai-desktop, else the start of the body.
fn error_message(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("error"))
        .and_then(|e| e.get("message").or(Some(e)))
        .and_then(Value::as_str)
        .map(str::to_string);
    message.unwrap_or_else(|| snippet(body))
}

fn snippet(text: &str) -> String {
    let t = text.trim();
    let cut: String = t.chars().take(SNIPPET).collect();
    if cut.len() < t.len() {
        format!("{cut}...")
    } else {
        cut
    }
}

/// A 200 answer's JSON, or the error its status stands for.
fn ok_json(status: u16, body: &str) -> Result<Value, AskError> {
    match status {
        200 => serde_json::from_str(body)
            .map_err(|e| AskError::Malformed(format!("{e}: {}", snippet(body)))),
        403 => Err(AskError::Forbidden(error_message(body))),
        500..=599 => Err(AskError::Backend {
            status,
            message: error_message(body),
        }),
        _ => Err(AskError::Rejected {
            status,
            message: error_message(body),
        }),
    }
}

fn parse_reply(status: u16, body: &str) -> Result<Reply, AskError> {
    let v = ok_json(status, body)?;
    let Some(text) = v.get("text").and_then(Value::as_str) else {
        return Err(AskError::Malformed(format!(
            "no `text` in the answer: {}",
            snippet(body)
        )));
    };
    let num = |k: &str| {
        v.get("usage")
            .and_then(|u| u.get(k))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let opt = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Ok(Reply {
        text: text.to_string(),
        provider: opt("provider"),
        stop_reason: opt("stop_reason"),
        usage: Usage {
            prompt_tokens: num("prompt_tokens"),
            completion_tokens: num("completion_tokens"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::super::fake::{Fake, Resp};
    use super::*;
    use std::time::Instant;

    fn req(timeout_ms: u64) -> AskRequest {
        AskRequest {
            prompt: "the prompt".into(),
            system: "the system".into(),
            model: "qwen2.5-coder".into(),
            timeout_ms,
        }
    }

    fn client(fake: &Fake) -> Client {
        Client::new(&fake.url())
            .unwrap()
            .with_slack(Duration::from_millis(300))
    }

    #[test]
    fn the_model_name_decides_the_backend() {
        for (model, backend) in [
            ("claude-sonnet-5", Backend::Anthropic),
            ("gpt-5", Backend::Openai),
            ("o1-mini", Backend::Openai),
            ("o3-pro", Backend::Openai),
            ("gemini-3-pro", Backend::Gemini),
            ("qwen2.5-coder", Backend::Ollama),
            ("llama3:8b", Backend::Ollama),
            ("", Backend::Ollama),
            ("my-claude-ish", Backend::Ollama),
        ] {
            assert_eq!(Backend::resolve(model), backend, "{model}");
        }
        assert!(Backend::Ollama.is_local());
        assert!(!Backend::Anthropic.is_local());
    }

    #[test]
    fn an_empty_allow_list_means_local_only() {
        let d = destination("qwen2.5-coder", &[]).unwrap();
        assert!(d.local);
        assert_eq!(d.backend, Backend::Ollama);
        let e = destination("claude-sonnet-5", &[]).unwrap_err();
        assert_eq!(e.key(), "err.agent.localOnly");
        assert_eq!(e.param("backend"), Some("anthropic"));
    }

    #[test]
    fn a_listed_backend_is_the_whole_allow_list() {
        let allow = vec!["Anthropic".to_string()];
        let d = destination("claude-sonnet-5", &allow).unwrap();
        assert_eq!(d.backend, Backend::Anthropic);
        assert!(!d.local);
        // Listing a cloud backend does not smuggle in the others, nor the local one.
        let e = destination("gpt-5", &allow).unwrap_err();
        assert_eq!(e.key(), "err.agent.notAllowed");
        assert_eq!(e.param("allowed"), Some("Anthropic"));
        assert_eq!(
            destination("llama3", &allow).unwrap_err().key(),
            "err.agent.notAllowed"
        );
        assert_eq!(
            destination("  ", &allow).unwrap_err().key(),
            "err.agent.model"
        );
    }

    #[test]
    fn the_address_must_name_this_machine() {
        assert_eq!(
            check_url("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
        assert_eq!(
            check_url(" http://localhost:9000/ ").unwrap(),
            "http://localhost:9000"
        );
        assert_eq!(check_url("http://[::1]:7").unwrap(), "http://[::1]:7");
        assert_eq!(
            check_url("http://localhost").unwrap(),
            "http://localhost:80"
        );
        for bad in [
            "",
            "127.0.0.1:8080",
            "https://127.0.0.1:8080",
            "http://example.com:8080",
            "http://127.0.0.1.example.com:8080",
            "http://user@127.0.0.1:8080",
            "http://127.0.0.1:8080/ask",
            "http://127.0.0.1:0",
            "http://127.0.0.1:99999",
            "http://127.0.0.1:x",
            "http://0.0.0.0:8080",
        ] {
            assert_eq!(check_url(bad).unwrap_err().key(), "err.agent.url", "{bad}");
        }
    }

    #[test]
    fn an_ask_sends_the_documented_body_and_no_origin() {
        let fake = Fake::start(|_| {
            Resp::json(
                200,
                r#"{"text":"hello","provider":"ai_desktop","stop_reason":"end_turn","usage":{"prompt_tokens":12,"completion_tokens":3}}"#,
            )
        });
        let reply = client(&fake).ask(&req(5000), &Stop::default()).unwrap();
        assert_eq!(reply.text, "hello");
        assert_eq!(reply.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(
            reply.usage,
            Usage {
                prompt_tokens: 12,
                completion_tokens: 3
            }
        );

        let seen = fake.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            (seen[0].method.as_str(), seen[0].path.as_str()),
            ("POST", "/ask")
        );
        assert_eq!(
            seen[0].header("origin"),
            None,
            "a browser sends one, this must not"
        );
        assert_eq!(seen[0].header("content-type"), Some("application/json"));
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(
            body,
            json!({"prompt":"the prompt","system":"the system","model":"qwen2.5-coder","timeout_ms":5000})
        );
    }

    #[test]
    fn health_reports_presence_only() {
        let fake = Fake::start(|_| {
            Resp::json(
                200,
                r#"{"status":"ready","backends":{"ollama":true,"openai":false,"anthropic":true,"gemini":false,"anthropic_key":"sk-secret"},"key":"sk-secret2"}"#,
            )
        });
        let h = client(&fake).health().unwrap();
        assert_eq!(h.status, "ready");
        assert_eq!(h.backends.len(), 4, "{h:?}");
        assert!(h.backends["anthropic"]);
        assert!(!h.backends["openai"]);
        assert!(!format!("{h:?}").contains("sk-secret"));
        assert_eq!(fake.seen()[0].path, "/health");
    }

    #[test]
    fn a_foreign_origin_answer_names_the_cause() {
        let fake = Fake::start(|_| {
            Resp::json(
                403,
                r#"{"error":{"message":"cross-origin request blocked"}}"#,
            )
        });
        let err = client(&fake).ask(&req(5000), &Stop::default()).unwrap_err();
        assert_eq!(
            err,
            AskError::Forbidden("cross-origin request blocked".into())
        );
        assert!(err.is_fatal());
        let app = err.into_app("http://127.0.0.1:8080");
        assert_eq!(app.key(), "err.agent.origin");
        assert_eq!(app.detail_text(), Some("cross-origin request blocked"));
    }

    #[test]
    fn a_backend_that_is_down_is_a_502_with_its_message() {
        let fake = Fake::start(|_| {
            Resp::json(502, r#"{"error":{"message":"ollama: connection refused"}}"#)
        });
        let err = client(&fake).ask(&req(5000), &Stop::default()).unwrap_err();
        assert_eq!(
            err,
            AskError::Backend {
                status: 502,
                message: "ollama: connection refused".into()
            }
        );
        assert!(
            !err.is_fatal(),
            "the next batch may go to a backend that is up"
        );
        let app = err.into_app("x");
        assert_eq!(app.key(), "err.agent.backend");
        assert_eq!(app.param("status"), Some("502"));
    }

    #[test]
    fn a_bad_request_is_rejected_not_retried() {
        let fake = Fake::start(|_| Resp::json(400, r#"{"error":{"message":"invalid request"}}"#));
        let err = client(&fake).ask(&req(5000), &Stop::default()).unwrap_err();
        assert!(matches!(err, AskError::Rejected { status: 400, .. }));
        assert!(err.is_fatal());
    }

    #[test]
    fn no_answer_in_time_is_a_timeout_that_says_how_long() {
        let fake = Fake::start(|_| Resp::json(200, r#"{"text":"late"}"#).after(3000));
        let started = Instant::now();
        let err = client(&fake).ask(&req(200), &Stop::default()).unwrap_err();
        assert_eq!(err, AskError::Timeout(200));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        let app = err.into_app("x");
        assert_eq!(app.key(), "err.agent.timeout");
        assert_eq!(app.param("secs"), Some("1"));
    }

    #[test]
    fn answers_that_are_not_the_promised_shape_are_malformed() {
        for body in [
            "not json at all",
            r#"{"nothing":1}"#,
            r#"{"text":42}"#,
            "[]",
        ] {
            let fake = Fake::start(move |_| Resp::json(200, body));
            let err = client(&fake).ask(&req(5000), &Stop::default()).unwrap_err();
            assert!(matches!(err, AskError::Malformed(_)), "{body}: {err:?}");
            assert_eq!(err.into_app("x").key(), "err.agent.malformed");
        }
    }

    #[test]
    fn nothing_listening_is_unreachable_and_names_the_address() {
        let url = {
            let fake = Fake::start(|_| Resp::json(200, "{}"));
            fake.url()
        };
        // The fake is dropped and its port closed.
        std::thread::sleep(Duration::from_millis(100));
        let err = Client::new(&url)
            .unwrap()
            .ask(&req(1000), &Stop::default())
            .unwrap_err();
        assert!(matches!(err, AskError::Unreachable(_)), "{err:?}");
        let app = err.into_app(&url);
        assert_eq!(app.key(), "err.agent.unreachable");
        assert_eq!(app.param("url"), Some(url.as_str()));
    }

    #[test]
    fn a_stop_abandons_the_request_in_flight() {
        let fake = Fake::start(|_| Resp::json(200, r#"{"text":"late"}"#).after(5000));
        let stop = std::sync::Arc::new(Stop::default());
        let s2 = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            s2.request(StopKind::Cancel);
        });
        let started = Instant::now();
        let err = client(&fake).ask(&req(20_000), &stop).unwrap_err();
        assert_eq!(err, AskError::Stopped(StopKind::Cancel));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }
}
