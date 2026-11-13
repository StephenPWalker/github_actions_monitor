//! A deliberately small HTTP/1.1 subset for a loopback-only app: one request per
//! connection, bounded headers/body, no transfer encoding and no file routing.
use crate::{connection, gh::Gh, snapshot};
use serde_json::json;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 16 * 1024;

pub struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

pub fn read_request(reader: &mut impl Read) -> Result<Request, &'static str> {
    let mut raw = Vec::new();
    while !raw.ends_with(b"\r\n\r\n") {
        if raw.len() >= MAX_HEADERS {
            return Err("Request headers too large.");
        }
        let mut byte = [0];
        reader
            .read_exact(&mut byte)
            .map_err(|_| "Incomplete request headers.")?;
        raw.push(byte[0]);
    }

    let text = std::str::from_utf8(&raw).map_err(|_| "Invalid request headers.")?;
    let mut lines = text.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();

    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return Err("Expected HTTP/1.1.");
    }

    let mut headers = HashMap::new();

    for line in lines.filter(|s| !s.is_empty()) {
        let (key, value) = line.split_once(':').ok_or("Invalid header.")?;
        let key = key.to_ascii_lowercase();
        if key.is_empty()
            || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || headers.insert(key, value.trim().to_owned()).is_some()
        {
            return Err("Invalid or repeated header.");
        }
    }

    if headers.contains_key("transfer-encoding") {
        return Err("Transfer encoding is unsupported.");
    }

    let length: usize = headers
        .get("content-length")
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| "Invalid content length.")?
        .unwrap_or(0);

    if length > MAX_BODY {
        return Err("Request body too large.");
    }

    let mut body: Vec<u8> = vec![0_u8; length];
    reader
        .read_exact(body.as_mut_slice())
        .map_err(|_| "Incomplete request body.")?;

    Ok(Request {
        method: parts[0].to_owned(),
        path: parts[1].to_owned(),
        headers,
        body,
    })
}

pub fn allowed(request: &Request, authority: &str) -> bool {
    request
        .headers
        .get("host")
        .is_some_and(|host| host == authority)
        && request
            .headers
            .get("origin")
            .is_none_or(|origin| origin == &format!("http://{authority}"))
        && request
            .headers
            .get("sec-fetch-site")
            .is_none_or(|site| site != "cross-site")
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'\r\n\r\n", body.len())?;
    stream.write_all(body)
}

fn serve(mut stream: TcpStream, authority: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(message) => {
            let _ = respond(
                &mut stream,
                "400 Bad Request",
                "text/plain; charset=utf-8",
                message.as_bytes(),
            );
            return;
        }
    };

    if !allowed(&request, authority) {
        let _ = respond(
            &mut stream,
            "403 Forbidden",
            "text/plain; charset=utf-8",
            b"Open this app using its printed loopback URL.",
        );
        return;
    }

    let asset = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => Some((
            "text/html; charset=utf-8",
            include_str!("../web/index.html"),
        )),
        ("GET", "/app.css") => Some(("text/css; charset=utf-8", include_str!("../web/app.css"))),
        ("GET", "/app.js") => Some((
            "text/javascript; charset=utf-8",
            include_str!("../web/app.js"),
        )),
        _ => None,
    };

    if let Some((kind, body)) = asset {
        let _ = respond(&mut stream, "200 OK", kind, body.as_bytes());
        return;
    }

    if request.method == "POST" && request.path == "/api/snapshot" {
        if request.headers.get("x-monitor-request").map(String::as_str) != Some("1")
            || request.headers.get("content-type").map(String::as_str) != Some("application/json")
        {
            let _ = respond(
                &mut stream,
                "403 Forbidden",
                "application/json",
                br#"{"error":"Expected a same-origin JSON monitor request."}"#,
            );
            return;
        }
        let result = serde_json::from_slice(&request.body)
            .map_err(|_| "Invalid JSON request.".to_owned())
            .and_then(|value| connection(&value))
            .and_then(|(username, token, repos)| {
                snapshot(&Gh { token: &token }, &username, &repos)
            });
        let (status, body) = match result {
            Ok(value) => ("200 OK", value),
            Err(error) => ("400 Bad Request", json!({"error":error})),
        };
        let _ = respond(
            &mut stream,
            status,
            "application/json; charset=utf-8",
            body.to_string().as_bytes(),
        );
    } else {
        let _ = respond(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Not found.",
        );
    }
}

pub fn run(port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let authority = format!("127.0.0.1:{}", listener.local_addr()?.port());
    println!("GitHub Actions Monitor: http://{authority}\nPress Ctrl+C to stop.");
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let mut stream = stream?;
        if active
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < 8).then_some(n + 1)
            })
            .is_err()
        {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            let _ = respond(
                &mut stream,
                "503 Service Unavailable",
                "application/json",
                br#"{"error":"Monitor is busy. Wait for the current refresh."}"#,
            );
            continue;
        }
        let active = Arc::clone(&active);
        let authority = authority.clone();
        thread::spawn(move || {
            struct Guard(Arc<AtomicUsize>);
            impl Drop for Guard {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                }
            }
            let _guard = Guard(active);
            serve(stream, &authority);
        });
    }

    Ok(())
}
