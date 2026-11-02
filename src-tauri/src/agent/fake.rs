//! A fake ai-desktop for the tests: a loopback HTTP server that records what it
//! was asked and answers from a closure.
//!
//! Hand-rolled over `TcpListener`, like the page host in `server.rs`: the
//! tests need exactly one thing (a request in, a canned response out, after a
//! delay if asked), and a mock-server crate would be a dependency for that.
//! Every connection gets its own thread, so a reply that is held back for
//! seconds (the timeout and cancel tests) does not block the next request.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One request as the server read it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// One response to send.
pub struct Resp {
    status: u16,
    body: String,
    delay_ms: u64,
}

impl Resp {
    pub fn json(status: u16, body: &str) -> Resp {
        Resp {
            status,
            body: body.to_string(),
            delay_ms: 0,
        }
    }

    /// Hold the response back this long first.
    pub fn after(mut self, ms: u64) -> Resp {
        self.delay_ms = ms;
        self
    }
}

pub struct Fake {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    done: Arc<AtomicBool>,
}

impl Fake {
    /// Listen on a free loopback port; `answer` decides every response.
    pub fn start(answer: impl Fn(&Seen) -> Resp + Send + Sync + 'static) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let answer = Arc::new(answer);
        {
            let (seen, done) = (seen.clone(), done.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let (seen, answer) = (seen.clone(), answer.clone());
                            std::thread::spawn(move || serve(stream, &seen, &*answer));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            });
        }
        Fake { port, seen, done }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// How many `POST /ask` arrived.
    pub fn asks(&self) -> usize {
        self.seen().iter().filter(|s| s.path == "/ask").count()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
    }
}

fn serve(stream: TcpStream, seen: &Mutex<Vec<Seen>>, answer: &dyn Fn(&Seen) -> Resp) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(clone);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    let mut length = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut body = vec![0u8; length];
    let _ = reader.read_exact(&mut body);
    let request = Seen {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    seen.lock().unwrap().push(request.clone());

    let resp = answer(&request);
    if resp.delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(resp.delay_ms));
    }
    let head = format!(
        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        resp.status,
        resp.body.len()
    );
    let mut stream = stream;
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(resp.body.as_bytes());
    let _ = stream.flush();
}
