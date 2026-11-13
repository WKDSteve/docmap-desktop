//! A small HTTP server for one project's map, so the page's own `/api/*`
//! panels have something on the other end.
//!
//! # Why this exists
//!
//! The window used to load the map through Tauri's asset protocol
//! (`convertFileSrc`). That shows the page but answers no request: the
//! generated page fetches `/api/telemetry`, `/api/loaded`, `/api/checklist`,
//! `/api/commits`, `/api/commit/<sha>`, `/api/shape/<sha>[/<sha>]` and two snapshot-listing routes, and
//! nothing was listening. On Windows it was worse than silent —
//! `convertFileSrc` hands out `http://asset.localhost/…`, so the page's own
//! "is a server behind this origin" test said *yes*, fetched, got the asset
//! host's 404, and advised running `:DocMap serve`, which does not help and
//! may already have been running. That misleading message is what started
//! this.
//!
//! # What is Rust's half and what is not
//!
//! Transport only. Every answer comes from the engine
//! (`docmap.nvim`'s standalone binary) via `--api=<route>`, which is
//! the same `core/api.lua` the in-editor server answers from. The join logic
//! stays in Lua, owned once. Two hosts computing the same join separately is
//! how they would come to disagree about one project while both looked
//! healthy.
//!
//! # Security posture
//!
//! Deliberately the same three rules `docmap.nvim`'s own
//! `editor/serve.lua` enforces, because this is the same surface — a socket
//! and a subprocess — pointed at the same data:
//!
//! 1. **Bind `127.0.0.1`, never `0.0.0.0`.** A personal viewer on a personal
//!    machine; the network has no business reaching a local repository's
//!    source.
//! 2. **Whitelist the route before it reaches the subprocess.** A request
//!    path is otherwise argument injection into the engine. Fixed-name
//!    routes go through a literal list; `commit/<sha>` and `shape/<sha>[/<sha>]`
//!    go through `safe_sha`, the same `^[0-9a-f]{7,40}$` shape check
//!    `docmap.core.api.safe_sha` enforces on the Lua side. Both are
//!    whitelists, not escapes.
//! 3. **No path parameter can leave the map directory.** Static serving takes
//!    a bare filename; anything with a separator or a `..` is refused before
//!    it becomes a path.
//! 4. **Only this page may ask.** `Host` must name this server and an `Origin`, if
//!    sent, must be this server too: a page elsewhere that rebinds a DNS name to
//!    127.0.0.1 is refused (DNS rebinding).
//! 5. **A client cannot hold it up.** The head of a request has a size and a
//!    time limit, and the connections at one time have a number.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::apperr::AppError;

/// Everything one running server answers for.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    /// The project root, passed to the engine as its scan root.
    pub root: String,
    /// Where the generated page lives.
    pub map_dir: String,
    /// The engine binary. Without it the API routes cannot be answered at
    /// all, and the server says so rather than 404ing like a missing file.
    pub engine: Option<String>,
    pub grammars: Option<String>,
}

pub struct Server {
    pub port: u16,
    pub root: String,
    /// Set to tell this server's accept loop to exit -- see `start`'s own
    /// polling loop for why a plain `TcpListener::incoming()` can't just be
    /// interrupted from outside.
    stop: Arc<AtomicBool>,
}

/// The one running server, if any. There is one window and one selected
/// project, so a second server would only be a socket nobody reads.
static CURRENT: Mutex<Option<Server>> = Mutex::new(None);

/// Take the server slot. A poisoned slot (a panic while it was held) is a
/// map that cannot be served, and says so: `err.lock` speaks of saving,
/// which is what the workspace lock guards, not what failed here.
fn lock_servers<T>(slot: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, AppError> {
    slot.lock()
        .map_err(|_| AppError::new("err.server.start").detail("server lock poisoned"))
}

/// Fixed-name routes the engine is allowed to be asked for. `commit/<sha>`
/// is not in here — see `handle`'s own dispatch — because its name carries
/// the sha, which cannot be a fixed list entry.
///
/// Mirrors `docmap.core.api`'s own `routes` table. Duplicated here on
/// purpose and kept short: this is the whitelist that stands between a
/// request path and a subprocess argument, and a whitelist that asks another
/// process what it should contain is not one.
const ROUTES: &[&str] = &[
    "telemetry",
    "telemetry/snapshots",
    "loaded",
    "loaded/snapshots",
    "checklist",
    "commits",
];

/// A commit-ish the caller supplied, or `None` if it is not one.
///
/// Mirrors `docmap.core.api.safe_sha` exactly (`^[0-9a-f]{7,40}$`) —
/// this is the second lock on the same door, checked here so an invalid sha
/// never even reaches the subprocess call, and again by the engine itself
/// once it does.
fn safe_sha(s: &str) -> bool {
    let n = s.len();
    // Lua's `%x` pattern class matches both cases, so this does too.
    (7..=40).contains(&n) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `shape/<a>` or `shape/<a>/<b>` (the Hierarchy tab's "Compare maps", answered by
/// `docmap.core.api.shape`), or `false`.
///
/// Every sha is checked with `safe_sha` on its own, and nothing but one or two of them
/// may follow the prefix: a third segment, an empty one or git's `a..b` spelling is not
/// this route. The name is then handed to the engine unchanged, like `commit/<sha>`.
fn safe_shape_route(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("shape/") else {
        return false;
    };
    let mut parts = rest.split('/');
    let first = parts.next().unwrap_or("");
    let second = parts.next();
    if parts.next().is_some() || !safe_sha(first) {
        return false;
    }
    second.is_none_or(safe_sha)
}

/// A request path segment is servable only if it is a bare filename.
///
/// An empty path means `index.html`; anything containing a separator or a
/// `..` is refused, so this cannot be walked out of the map directory.
fn safe_static_name(name: &str) -> Option<&str> {
    if name.is_empty() {
        return Some("index.html");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    Some(name)
}

/// The file a request names inside the map directory, or `None`.
///
/// What is served is the *repository's* `docs/map`, and the page in it runs in
/// a frame the same origin as this server, so whatever this reads it can hand
/// to that page's script. The name is already a bare filename
/// ([`safe_static_name`]); this adds what a name cannot say:
///
/// * **One plain component.** On Windows `C:secret.txt` has a drive prefix and
///   `Path::join` replaces the base with it; `a:stream` names an NTFS stream.
/// * **The resolved file stays inside the map directory.** A symlink that git
///   checked out as `docs/map/leak -> ~/.ssh/id_rsa` resolves outside and is
///   refused, and so is a junction or a link to a device.
/// * **The map directory is what it says it is** — see
///   [`crate::map_dir_is_plain`]: `docs/map` checked out as a link to `..`
///   would make "inside the map directory" mean "anywhere in the repository",
///   `.env` included.
/// * **The file is looked at before it is opened** — see [`crate::map_file`]:
///   a link to another machine is refused without being followed.
fn servable_file(root: &Path, map_dir: &Path, name: &str) -> Option<PathBuf> {
    let mut parts = Path::new(name).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return None;
    }
    if name.contains(':') || name.contains('\0') {
        return None;
    }
    // The directory and the file both: `docs/map/leak` can be a link to a
    // share as well, and `canonicalize` would connect to it before the
    // `starts_with` below could say no. `map_file` walks it without following.
    let candidate = crate::map_file(root, map_dir, name)?;
    let base = std::fs::canonicalize(map_dir).ok()?;
    let file = std::fs::canonicalize(candidate).ok()?;
    file.starts_with(&base).then_some(file)
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "css" => "text/css",
        "js" => "text/javascript",
        "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn respond(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// A `/api/*` (or static) failure. The body keeps `error`, the English
/// sentence the generated page reads and shows; `key`, `params` and `detail`
/// ride along in the shape every command's error has, under `api.*` keys that
/// belong to the page's catalog and never to this app's. See `apperr.rs`.
fn respond_json_error(stream: &mut TcpStream, code: u16, error: &AppError) {
    respond(
        stream,
        code,
        "application/json",
        error.api_body().as_bytes(),
    );
}

#[cfg(windows)]
pub(crate) fn no_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn no_window(_cmd: &mut std::process::Command) {}

/// Does this engine understand `--api=`?
///
/// **This check is not optional, and asking the obvious way would be
/// destructive.** An engine built before `--api=` existed treats it as an
/// ordinary argument and *generates the map* — writing into the user's
/// repository and exiting 0 with output that is not JSON. A panel that polls
/// would rewrite their `docs/map` on every fetch. Measured against the
/// shipped binary, not imagined.
///
/// `--capabilities` is safe against both versions precisely because it sits
/// in the root-argument position, which an older binary rejects with exit 2
/// before doing any work. So a failed probe means "too old" and "no API" at
/// once, which is the only thing this caller needs to distinguish.
fn engine_supports_api(engine: &str) -> bool {
    let mut cmd = std::process::Command::new(engine);
    cmd.arg("--capabilities");
    no_window(&mut cmd);
    let Ok(out) = cmd.output() else { return false };
    if !out.status.success() {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .ok()
        .and_then(|v| {
            v.get("capabilities")
                .and_then(|c| c.as_array())
                .map(|c| c.iter().any(|x| x.as_str() == Some("api")))
        })
        .unwrap_or(false)
}

/// Ask the engine for one route's JSON.
///
/// `param` is `standalone/docmap.lua`'s single `--snapshot=` flag, read by
/// whichever route asked for it as a snapshot name (telemetry/loaded) or a
/// commit count (commits) — the same one-flag-many-meanings shape
/// `core/api.lua`'s own `M.answer(name, opts, param)` already uses, kept
/// consistent on this side rather than inventing a second flag per route.
fn engine_api(cfg: &ServeConfig, route: &str, param: Option<&str>) -> Result<String, AppError> {
    let engine = cfg
        .engine
        .as_ref()
        .ok_or_else(|| AppError::new("api.engine.none"))?;

    if !engine_supports_api(engine) {
        return Err(AppError::new("api.engine.old").with("engine", engine));
    }

    let mut cmd = std::process::Command::new(engine);
    cmd.arg(&cfg.root).arg(format!("--api={route}"));
    if let Some(p) = param {
        cmd.arg(format!("--snapshot={p}"));
    }
    if let Some(g) = &cfg.grammars {
        cmd.env("DOCMAP_TS_DIR", g);
    }
    no_window(&mut cmd);

    let out = cmd.output().map_err(|e| {
        AppError::new("api.engine.run")
            .with("engine", engine)
            .detail(e)
    })?;
    if !out.status.success() {
        return Err(AppError::new("api.engine.exit")
            .with("code", out.status.code().unwrap_or(-1))
            .detail(String::from_utf8_lossy(&out.stderr)));
    }

    let body = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // Parsed before it is passed on, even though it is forwarded verbatim:
    // the capability check above already rules out the engine that answers
    // an API request with a generation report, and this is the second lock
    // on that same door. Serving prose as `application/json` turns a
    // recognisable failure into a parse error inside the page.
    if serde_json::from_str::<serde_json::Value>(&body).is_err() {
        return Err(AppError::new("api.engine.notJson").with("route", route));
    }
    Ok(body)
}

/// The most bytes of a request head (the request line and the headers).
const MAX_HEAD: u64 = 16 * 1024;
/// How long a client has to send its head, and to take an answer.
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// The most connections at one time. A page asks a handful of things at once.
const MAX_CONNECTIONS: usize = 32;

/// A socket with one deadline for everything read from it: a read timeout alone starts anew with every byte, so one byte
/// every few seconds would hold a connection for hours.
struct UntilDeadline {
    stream: TcpStream,
    end: Instant,
}

impl Read for UntilDeadline {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

/// The lines of a request head, without the blank line that ends it. `None`: it is
/// over [`MAX_HEAD`], not text, or ended before the blank line.
fn read_head(reader: impl BufRead) -> Option<Vec<String>> {
    let mut limited = reader.take(MAX_HEAD);
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        let got = limited.read_line(&mut line).ok()?;
        if got == 0 || !line.ends_with('\n') {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            return Some(lines);
        }
        lines.push(line.to_owned());
    }
}

/// The one value of a header, by its name in any case; `None` when it is not there,
/// and when it is there twice nothing can tell which one is meant, so that is `Err`.
fn one_header<'a>(head: &'a [String], name: &str) -> Result<Option<&'a str>, ()> {
    let mut found = None;
    for line in head.iter().skip(1) {
        if let Some((key, value)) = line.split_once(':') {
            if key.trim().eq_ignore_ascii_case(name) {
                if found.is_some() {
                    return Err(());
                }
                found = Some(value.trim());
            }
        }
    }
    Ok(found)
}

/// Whether the request comes from this server's own page: `Host` names it, and an
/// `Origin` (a browser sends it on some requests) does too.
fn asked_by_own_page(head: &[String], port: u16) -> bool {
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let Ok(Some(host)) = one_header(head, "host") else {
        return false;
    };
    if !hosts.iter().any(|h| h == host) {
        return false;
    }
    match one_header(head, "origin") {
        Ok(None) => true,
        Ok(Some(origin)) => hosts.iter().any(|h| origin == format!("http://{h}")),
        Err(()) => false,
    }
}

/// Whether the page's own fetches are asking. Loading the page itself comes from the window, another origin; but a route
/// of the API costs engine processes, and a page elsewhere must not be able to start them with an `<img>` (such a request
/// sends no `Origin`). A client that sends no `Sec-Fetch-Site` is no browser.
fn api_asked_by_own_page(head: &[String]) -> bool {
    matches!(
        one_header(head, "sec-fetch-site"),
        Ok(None | Some("same-origin"))
    )
}

fn handle(cfg: &ServeConfig, port: u16, stream: &mut TcpStream) {
    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return;
    }
    let reader = BufReader::new(UntilDeadline {
        stream: match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        },
        end: Instant::now() + IO_TIMEOUT,
    });
    let Some(head) = read_head(reader) else {
        return respond_json_error(stream, 400, &AppError::new("api.badRequest"));
    };
    if !asked_by_own_page(&head, port) {
        return respond_json_error(stream, 403, &AppError::new("api.foreign"));
    }
    let line = head.first().map(String::as_str).unwrap_or("");

    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method != "GET" {
        return respond_json_error(stream, 405, &AppError::new("api.method"));
    }

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };

    // The page's own "is a docmap server behind this origin" probe. Answering
    // it is what stops the four server-backed panels from reporting the
    // misleading `:DocMap serve` advice described in this module's header.
    if path == "/api/ping" {
        return respond(
            stream,
            200,
            "application/json",
            br#"{"docmap":true}"#.as_slice(),
        );
    }

    if let Some(name) = path.strip_prefix("/api/") {
        if !api_asked_by_own_page(&head) {
            return respond_json_error(stream, 403, &AppError::new("api.foreign"));
        }
        // `commit/<sha>` first, same reason `core.api.answer` checks it
        // before its own fixed route table: the sha varies, so it cannot
        // be a `ROUTES` entry, and it needs its own validation rather than
        // falling through to the "no such endpoint" branch below.
        if let Some(sha) = name.strip_prefix("commit/") {
            if !safe_sha(sha) {
                return respond_json_error(stream, 400, &AppError::new("api.sha"));
            }
            return match engine_api(cfg, name, None) {
                Ok(body) => respond(stream, 200, "application/json", body.as_bytes()),
                Err(e) => respond_json_error(stream, 500, &e),
            };
        }
        // `shape/<a>[/<b>]`: the same reasoning, for one or two shas. The
        // engine answers it from `core/diff.lua`; nothing but hex reaches it.
        if name.starts_with("shape/") {
            if !safe_shape_route(name) {
                return respond_json_error(stream, 400, &AppError::new("api.sha"));
            }
            return match engine_api(cfg, name, None) {
                Ok(body) => respond(stream, 200, "application/json", body.as_bytes()),
                Err(e) => respond_json_error(stream, 500, &e),
            };
        }

        if !ROUTES.contains(&name) {
            // Deliberately not echoed back: refusing to repeat an
            // unvalidated value is free, and this is the input that would
            // otherwise reach a subprocess.
            return respond_json_error(stream, 404, &AppError::new("api.route"));
        }
        // `snapshot=` (telemetry/loaded) and `n=` (commits) never both
        // apply to the same route, mirroring `editor/serve.lua`'s own
        // `respond_api` — trying both and taking whichever is present
        // needs no per-route branch here either.
        let param = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("snapshot="))
            .or_else(|| query.split('&').find_map(|kv| kv.strip_prefix("n=")));
        return match engine_api(cfg, name, param) {
            // Passed through verbatim: the engine already produced the exact
            // JSON the page expects, including its `available:false` +
            // `reason` shape for "nothing to show". Re-encoding it here would
            // add a second opinion about a contract that already has one.
            Ok(body) => respond(stream, 200, "application/json", body.as_bytes()),
            Err(e) => respond_json_error(stream, 500, &e),
        };
    }

    let requested = path.trim_start_matches('/');
    let Some(name) = safe_static_name(requested) else {
        return respond_json_error(stream, 400, &AppError::new("api.path"));
    };
    let body = servable_file(Path::new(&cfg.root), Path::new(&cfg.map_dir), name)
        .and_then(|file| crate::safe_read::read_bytes(&file, crate::safe_read::MAP_SERVE_MAX));
    match body {
        Some(body) => respond(stream, 200, content_type(name), &body),
        None => respond_json_error(stream, 404, &AppError::new("api.notFound")),
    }
}

/// Start serving `cfg`, returning the port.
///
/// Idempotent per project, and restarting when the project changes — the
/// same rule `editor/serve.lua` arrived at after a real bug: a server that
/// keeps answering for the previously selected repository does not look like
/// the wrong tree, it looks like a broken map.
pub fn start(cfg: ServeConfig) -> Result<u16, AppError> {
    let mut current = lock_servers(&CURRENT)?;
    if let Some(running) = current.as_ref() {
        if running.root == cfg.root {
            return Ok(running.port);
        }
        // Switching projects: tell the previous listener's accept loop
        // (below) to exit on its next poll instead of leaking it -- it
        // would otherwise keep a thread and a bound socket alive for the
        // rest of the app's lifetime, once per distinct project ever
        // viewed in the session.
        running.stop.store(true, Ordering::Relaxed);
    }

    // Port 0: the OS picks a free one. A hardcoded port would collide with
    // something eventually, and a retry loop looking for a free one is the
    // same thing done worse.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let listener =
        TcpListener::bind(addr).map_err(|e| AppError::new("err.server.start").detail(e))?;
    // Non-blocking so the accept loop can poll `stop` instead of blocking in
    // `incoming()` forever -- std's TcpListener has no other way to cancel
    // an in-progress accept from outside the thread that owns it.
    listener
        .set_nonblocking(true)
        .map_err(|e| AppError::new("err.server.start").detail(e))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::new("err.server.start").detail(e))?
        .port();

    let stop = Arc::new(AtomicBool::new(false));
    let shared_cfg = Arc::new(cfg.clone());
    let open = Arc::new(AtomicUsize::new(0));
    let shared_stop = Arc::clone(&stop);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if shared_stop.load(Ordering::Relaxed) {
                break;
            }
            match stream {
                Ok(mut stream) => {
                    // Accepted sockets don't reliably inherit the listener's
                    // non-blocking mode across platforms -- set it back
                    // explicitly so `handle`'s blocking reads/writes behave
                    // the same as before this loop became pollable.
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        open.fetch_sub(1, Ordering::SeqCst);
                        // Closed unanswered: writing could block this loop.
                        continue;
                    }
                    let cfg = Arc::clone(&shared_cfg);
                    let open = Arc::clone(&open);
                    // One thread per connection: `Connection: close`, a handful
                    // of requests per project, and an engine call that takes
                    // real time and must not block the next request behind it.
                    std::thread::spawn(move || {
                        handle(&cfg, port, &mut stream);
                        open.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    });

    *current = Some(Server {
        port,
        root: cfg.root.clone(),
        stop,
    });
    Ok(port)
}

/// Where a served project's page lives.
pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

#[cfg(test)]
mod tests {
    use super::{
        api_asked_by_own_page, asked_by_own_page, lock_servers, read_head, safe_sha,
        safe_shape_route, servable_file, UntilDeadline, MAX_HEAD,
    };
    use std::fs;
    use std::sync::Mutex;

    #[test]
    fn a_poisoned_server_slot_says_the_map_cannot_be_served() {
        let slot = Mutex::new(0u8);
        let _ = std::panic::catch_unwind(|| {
            let _held = slot.lock().unwrap();
            panic!("poison the slot");
        });
        assert!(slot.is_poisoned());
        let e = lock_servers(&slot).expect_err("a poisoned slot is an error");
        assert_eq!(e.key(), "err.server.start");
        assert_eq!(e.detail_text(), Some("server lock poisoned"));
        assert!(lock_servers(&Mutex::new(0u8)).is_ok());
    }

    // The one function here worth a real test even in a crate with no
    // CI: it is the whole security property standing between a request
    // path and a subprocess argument, and `docmap.core.api`'s own
    // `safe_sha`'s doc comment says exactly why that kind of function does
    // not get tested unless something actually exercises it — this is
    // that something, for the Rust half.
    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("docmap-serve-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A project root with `docs/map` under it.
    fn project(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = tmp(name);
        let map = root.join("docs/map");
        fs::create_dir_all(&map).unwrap();
        (root, map)
    }

    fn head(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| (*l).to_owned()).collect()
    }

    #[test]
    fn only_this_servers_own_page_may_ask() {
        let get = "GET /api/ping HTTP/1.1";
        for ok in [
            head(&[get, "Host: 127.0.0.1:5000"]),
            head(&[get, "host: localhost:5000"]),
            head(&[get, "Host: 127.0.0.1:5000", "Origin: http://127.0.0.1:5000"]),
        ] {
            assert!(asked_by_own_page(&ok, 5000), "{ok:?}");
        }
        for bad in [
            head(&[get]),
            head(&[get, "Host: evil.example:5000"]),
            head(&[get, "Host: 127.0.0.1:5001"]),
            head(&[get, "Host: 127.0.0.1:5000", "Host: evil.example"]),
            head(&[get, "Host: 127.0.0.1:5000", "Origin: https://evil.example"]),
            head(&[get, "Host: 127.0.0.1:5000", "Origin: null"]),
        ] {
            assert!(!asked_by_own_page(&bad, 5000), "{bad:?}");
        }
    }

    #[test]
    fn the_api_is_for_the_pages_own_fetches() {
        let get = "GET /api/commits HTTP/1.1";
        assert!(api_asked_by_own_page(&head(&[get])));
        assert!(api_asked_by_own_page(&head(&[
            get,
            "Sec-Fetch-Site: same-origin"
        ])));
        assert!(!api_asked_by_own_page(&head(&[
            get,
            "Sec-Fetch-Site: cross-site"
        ])));
        assert!(!api_asked_by_own_page(&head(&[
            get,
            "Sec-Fetch-Site: same-site"
        ])));
    }

    #[test]
    fn a_client_that_drips_bytes_meets_one_deadline() {
        use std::io::{BufReader, Write};
        use std::time::{Duration, Instant};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        // A byte every 100 ms: each read alone would be well within any timeout.
        let client = std::thread::spawn(move || {
            let mut out = std::net::TcpStream::connect(addr).unwrap();
            for _ in 0..30 {
                if out.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let (stream, _) = listener.accept().unwrap();
        let started = Instant::now();
        let reader = BufReader::new(UntilDeadline {
            stream,
            end: started + Duration::from_millis(500),
        });
        assert!(read_head(reader).is_none());
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
        client.join().unwrap();
    }

    #[test]
    fn a_head_has_a_size_and_an_end() {
        let good = b"GET / HTTP/1.1\r\nHost: x\r\n\r\nbody".as_slice();
        assert_eq!(read_head(good).unwrap(), ["GET / HTTP/1.1", "Host: x"]);
        assert!(read_head(b"GET / HTTP/1.1\r\nHost".as_slice()).is_none());
        let long = "x".repeat(usize::try_from(MAX_HEAD).unwrap() + 1);
        assert!(read_head(long.as_bytes()).is_none());
        assert!(read_head(&[0xff, b'\n'][..]).is_none());
    }

    /// The status of what a real server answers to `request`.
    fn status_of(port: u16, request: &str) -> String {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        std::io::Write::write_all(&mut stream, request.as_bytes()).unwrap();
        let mut answer = String::new();
        let _ = std::io::Read::read_to_string(&mut stream, &mut answer);
        answer.lines().next().unwrap_or("").to_owned()
    }

    #[test]
    fn a_real_server_answers_its_own_page_and_refuses_another_host() {
        let (root, map) = project("real-server");
        let port = super::start(super::ServeConfig {
            root: root.display().to_string(),
            map_dir: map.display().to_string(),
            engine: None,
            grammars: None,
        })
        .unwrap();
        let own = format!(
            "GET /api/ping HTTP/1.1
Host: 127.0.0.1:{port}

"
        );
        assert!(status_of(port, &own).contains("200"));
        let foreign = "GET /api/ping HTTP/1.1
Host: evil.example

";
        assert!(status_of(port, foreign).contains("403"));
        assert!(status_of(
            port,
            "nonsense

"
        )
        .contains("403"));
    }

    #[test]
    fn a_plain_file_in_the_map_directory_is_served() {
        let (root, map) = project("plain");
        fs::write(map.join("index.html"), "x").unwrap();
        assert!(servable_file(&root, &map, "index.html").is_some());
        assert!(servable_file(&root, &map, "missing.js").is_none());
    }

    #[test]
    fn a_drive_prefix_or_a_stream_is_never_a_name() {
        let (root, map) = project("prefix");
        fs::write(map.join("a.txt"), "x").unwrap();
        // A real NUL byte: a name with one is refused whatever else is true.
        for name in [
            "C:secret.txt",
            "a.txt:stream",
            "a.txt::$DATA",
            "a\0.txt",
            "",
        ] {
            assert!(servable_file(&root, &map, name).is_none(), "{name:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_resolves_outside_the_map_directory_is_refused() {
        let (root, map) = project("link");
        let outside = tmp("link-outside");
        fs::write(outside.join("secret"), "s").unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), map.join("leak")).unwrap();
        assert!(servable_file(&root, &map, "leak").is_none());
    }

    #[test]
    fn a_map_directory_outside_the_project_is_not_served() {
        // `../maps`, as the Map directory field keeps it.
        let outer = tmp("outside");
        let proj = outer.join("proj");
        fs::create_dir_all(&proj).unwrap();
        fs::create_dir_all(outer.join("maps")).unwrap();
        fs::write(outer.join("maps/index.html"), "x").unwrap();
        assert!(servable_file(&proj, &proj.join("../maps"), "index.html").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_as_the_map_directory_is_not_served_from() {
        // A junction needs no privilege, unlike a symlink: this runs wherever
        // the tests do.
        let root = tmp("junction");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join(".env"), "SECRET=1").unwrap();
        assert!(crate::testutil::dir_link(
            &root.join("docs").join("map"),
            &root
        ));
        assert!(servable_file(&root, &root.join("docs").join("map"), ".env").is_none());
    }

    #[test]
    fn a_file_that_links_out_of_the_map_directory_is_not_served() {
        let root = crate::testutil::fresh_dir("docmap-serve-filelink");
        let map = root.join("docs").join("map");
        fs::create_dir_all(&map).unwrap();
        fs::write(map.join("index.html"), "x").unwrap();
        fs::write(root.join(".env"), "SECRET=1").unwrap();
        assert!(servable_file(&root, &map, "index.html").is_some());
        if !crate::testutil::file_link(&map.join("leak"), &root.join(".env")) {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        // It stays inside the project but leaves the map directory.
        assert!(servable_file(&root, &map, "leak").is_none());
    }

    /// The file is looked at before it is opened: a link to a share must not
    /// make the server connect there just to find out it leaves the map
    /// directory.
    #[cfg(windows)]
    #[test]
    fn a_file_that_links_to_another_machine_is_refused_without_waiting() {
        let root = crate::testutil::fresh_dir("docmap-serve-unc");
        let map = root.join("docs").join("map");
        fs::create_dir_all(&map).unwrap();
        let target = format!(r"\\{}\share\leak", crate::testutil::unc_host());
        if !crate::testutil::file_link(&map.join("leak"), std::path::Path::new(&target)) {
            eprintln!("SKIP: no privilege to create symlinks");
            return;
        }
        let started = std::time::Instant::now();
        assert!(servable_file(&root, &map, "leak").is_none());
        assert!(
            started.elapsed().as_secs() < 3,
            "it must not wait for a host"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_map_directory_that_is_itself_a_link_is_not_served_from() {
        // `docs/map -> ..` made "inside the map directory" mean "anywhere in
        // the repository": a page could fetch `/.env`.
        let root = tmp("maplink");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join(".env"), "SECRET=1").unwrap();
        std::os::unix::fs::symlink("..", root.join("docs/map")).unwrap();
        assert!(servable_file(&root, &root.join("docs/map"), ".env").is_none());
    }

    #[test]
    fn accepts_real_shas() {
        assert!(safe_sha("abc1234"));
        assert!(safe_sha(&"a".repeat(40)));
        assert!(safe_sha("ABC1234"));
    }

    #[test]
    fn refuses_shapes_that_are_not_a_sha() {
        assert!(!safe_sha("abc123")); // 6 chars, one short of the 7 minimum
        assert!(!safe_sha(&"a".repeat(41))); // one over the 40 maximum
        assert!(!safe_sha("")); // empty
        assert!(!safe_sha("HEAD")); // a real revision, but not this route's shape
        assert!(!safe_sha("g1234567")); // 'g' is not a hex digit
    }

    #[test]
    fn shape_route_takes_one_or_two_shas() {
        assert!(safe_shape_route("shape/abc1234"));
        assert!(safe_shape_route(&format!(
            "shape/{}/{}",
            "a".repeat(40),
            "b".repeat(7)
        )));
    }

    #[test]
    fn shape_route_refuses_everything_else() {
        assert!(!safe_shape_route("shape/"));
        assert!(!safe_shape_route("shape/abc1234/"));
        assert!(!safe_shape_route("shape//abc1234"));
        assert!(!safe_shape_route("shape/abc1234/bcd2345/cde3456")); // a third sha
        assert!(!safe_shape_route("shape/abc1234..bcd2345")); // git's spelling is not the route's
        assert!(!safe_shape_route("shape/HEAD"));
        assert!(!safe_shape_route("shape/abc1234/HEAD"));
        assert!(!safe_shape_route("shape/abc1234 --upload-pack=evil"));
        assert!(!safe_shape_route("commit/abc1234"));
        assert!(!safe_shape_route("shapes/abc1234"));
    }

    #[test]
    fn refuses_injection_shaped_input() {
        // The exact class of value `safe_sha` exists to keep away from a
        // subprocess argument.
        assert!(!safe_sha("abc1234; rm -rf /"));
        assert!(!safe_sha("abc1234 --upload-pack=evil"));
        assert!(!safe_sha("abc1234\" & calc.exe & \""));
    }
}
