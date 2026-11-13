use github_actions_monitor::http::{allowed, read_request, Request};

fn parse(text: &str) -> Result<Request, &'static str> {
    read_request(&mut text.as_bytes())
}

#[test]
fn rejects_ambiguous_and_oversized_requests() {
    for headers in [
        "Content-Length: 0\r\nContent-Length: 1",
        "Transfer-Encoding: chunked",
        "Content-Length: 999999",
        "Content-Length: -1",
    ] {
        assert!(parse(&format!(
            "POST /api/snapshot HTTP/1.1\r\nHost: 127.0.0.1:3000\r\n{headers}\r\n\r\n"
        ))
        .is_err());
    }
    assert!(parse("POST /api/snapshot HTTP/1.1\r\nContent-Length: 10\r\n\r\nshort").is_err());
}

#[test]
fn rejects_cross_origin_and_dns_rebinding_requests() {
    for headers in [
        "Host: attacker.example",
        "Host: 127.0.0.1:3000\r\nOrigin: https://attacker.example",
        "Host: 127.0.0.1:3000\r\nSec-Fetch-Site: cross-site",
    ] {
        assert!(!allowed(
            &parse(&format!("GET / HTTP/1.1\r\n{headers}\r\n\r\n")).unwrap(),
            "127.0.0.1:3000"
        ));
    }
    assert!(allowed(
        &parse("GET / HTTP/1.1\r\nHost: 127.0.0.1:3000\r\n\r\n").unwrap(),
        "127.0.0.1:3000"
    ));
}
