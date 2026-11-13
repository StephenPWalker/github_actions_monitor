use crate::{page_items, Api};
use serde_json::Value;
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Gh<'a> {
    pub token: &'a str,
}

fn read_bounded(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read gh output.")?;

    if bytes.len() > 16 * 1024 * 1024 {
        return Err("GitHub response exceeded 16 MiB. Select fewer repositories.".into());
    }

    Ok(bytes)
}

// Never expose CLI stderr: it may contain authentication or diagnostic details.
pub fn failure(stderr: &str) -> String {
    if stderr.contains("HTTP 401")
        || stderr.contains("not logged")
        || stderr.contains("gh auth login")
    {
        "Authentication failed. Run gh auth login --hostname github.com or enter your own valid token.".into()
    } else if stderr.contains("HTTP 429") || stderr.to_ascii_lowercase().contains("rate limit") {
        "GitHub rate limit reached. Pause monitoring, wait for your quota to reset, and select fewer repositories.".into()
    } else if stderr.contains("org admin") || stderr.contains("runners and runner groups") {
        "GitHub denied organization runner access (HTTP 403). Your account needs organization administrator access or the required runner-group permission. Token scopes alone cannot grant an organization role. Also check admin:org for CLI/classic tokens, or Self-hosted runners: read on an organization-owned fine-grained token.".into()
    } else if stderr.contains("HTTP 403") || stderr.contains("HTTP 404") {
        "GitHub denied access (HTTP 403/404). Repository runners need Administration: read; organization runners need Self-hosted runners: read (admin:org for CLI/classic tokens); jobs need Actions: read. Check organization approval and SSO.".into()
    } else {
        "GitHub CLI request failed. Check your network and gh auth status in a terminal.".into()
    }
}

impl Api for Gh<'_> {
    fn get(&self, endpoint: &str) -> Result<Value, String> {
        let mut command = Command::new("gh");
        command.args([
            "api",
            "--hostname",
            "github.com",
            "--method",
            "GET",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "X-GitHub-Api-Version: 2022-11-28",
            endpoint,
        ]);

        if endpoint != "user" {
            command.args(["--paginate", "--slurp"]);
        }

        command
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_HOST", "github.com")
            .env_remove("GH_DEBUG")
            .env_remove("GH_FORCE_TTY");

        if !self.token.is_empty() {
            command
                .env("GH_TOKEN", self.token)
                .env_remove("GITHUB_TOKEN");
        }

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|_| {
            "Could not start GitHub CLI. Install gh and ensure it is on PATH.".to_owned()
        })?;
        let out = child.stdout.take().ok_or("Could not capture gh output.")?;
        let err = child
            .stderr
            .take()
            .ok_or("Could not capture gh diagnostics.")?;
        let stdout = thread::spawn(move || read_bounded(out));
        let stderr = thread::spawn(move || read_bounded(err));
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if start.elapsed() < Duration::from_secs(20) => {
                    thread::sleep(Duration::from_millis(25))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(
                        "GitHub CLI timed out after 20 seconds. Check your connection.".to_owned(),
                    );
                }
            }
        };

        let out = stdout.join().map_err(|_| "Could not read gh output.")??;
        let err = stderr
            .join()
            .map_err(|_| "Could not read gh diagnostics.")??;

        if !status?.success() {
            return Err(failure(&String::from_utf8_lossy(&err)));
        }

        let value: Value =
            serde_json::from_slice(&out).map_err(|_| "GitHub CLI returned invalid JSON.")?;

        decode_response(endpoint, value)
    }
}

pub fn decode_response(endpoint: &str, value: Value) -> Result<Value, String> {
    if endpoint == "user" {
        return Ok(value);
    }
    let key = if endpoint.starts_with("user/repos?") || endpoint.starts_with("user/orgs?") {
        None
    } else if endpoint.contains("/runners?") {
        Some("runners")
    } else if endpoint.contains("/jobs?") {
        Some("jobs")
    } else {
        Some("workflow_runs")
    };

    Ok(Value::Array(page_items(value, key)?))
}
