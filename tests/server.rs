use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(address: &str, method: &str, path: &str, headers: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn real_server_serves_page_and_protects_credentials_without_contacting_github() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let address = format!("127.0.0.1:{port}");
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_github-actions-monitor"))
            .args(["--port", &port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut ready = false;
    for _ in 0..200 {
        if TcpStream::connect(&address).is_ok() {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready, "Server did not become ready");
    let html = request(&address, "GET", "/", "", "");
    assert!(html.starts_with("HTTP/1.1 200 OK"));
    assert!(html.contains("Your runners."));
    assert!(html.contains("Cache-Control: no-store"));
    assert!(html.contains("frame-ancestors 'none'"));
    for asset in ["/app.js", "/app.css"] {
        assert!(request(&address, "GET", asset, "", "").starts_with("HTTP/1.1 200 OK"));
    }
    assert!(request(&address, "GET", "/../../Cargo.toml", "", "").starts_with("HTTP/1.1 404"));
    let body = r#"{"username":"../invalid"}"#;
    assert!(request(
        &address,
        "POST",
        "/api/snapshot",
        "Content-Type: application/json\r\n",
        body
    )
    .starts_with("HTTP/1.1 403"));
    let headers = "Content-Type: application/json\r\nX-Monitor-Request: 1\r\n";
    let invalid = request(&address, "POST", "/api/snapshot", headers, body);
    assert!(invalid.starts_with("HTTP/1.1 400"));
    assert!(invalid.contains("Enter your GitHub username"));
    assert!(request(
        &address,
        "POST",
        "/api/snapshot",
        &format!("{headers}Origin: https://attacker.example\r\n"),
        body
    )
    .starts_with("HTTP/1.1 403"));
}
