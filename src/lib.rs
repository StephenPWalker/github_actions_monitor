use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub mod gh;
pub mod http;

pub trait Api {
    fn get(&self, endpoint: &str) -> Result<Value, String>;
}

pub fn connection(value: &Value) -> Result<(String, String, Vec<String>), String> {
    let username = value["username"].as_str().unwrap_or("").trim().to_owned();
    if username.is_empty()
        || username.len() > 39
        || !username
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err("Enter your GitHub username (letters, numbers and hyphens).".into());
    }

    let token = value["token"].as_str().unwrap_or("").trim().to_owned();
    if token.len() > 4096 || token.bytes().any(|c| c.is_ascii_control() || c == b' ') {
        return Err("The token contains invalid characters.".into());
    }

    let mut repos = Vec::new();
    for repo in value["repos"]
        .as_str()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let parts: Vec<_> = repo.split('/').collect();
        if parts.len() > 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || part.len() > 100
                    || *part == "."
                    || *part == ".."
                    || !part
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            })
            || (parts.len() == 2
                && !parts[0]
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-'))
        {
            return Err(
                "Use comma-separated repository names or owner/repository, without URLs.".into(),
            );
        }

        if !repos.iter().any(|r: &String| r.eq_ignore_ascii_case(repo)) {
            repos.push(repo.to_owned());
        }
    }

    Ok((username, token, repos))
}

pub fn snapshot(api: &impl Api, username: &str, filter: &[String]) -> Result<Value, String> {
    let user = api.get("user")?;
    let login = user["login"]
        .as_str()
        .ok_or("GitHub did not return an account.")?;

    if !login.eq_ignore_ascii_case(username) {
        return Err("The authenticated account does not match your username. Switch gh accounts or supply that account's token.".into());
    }

    let mut warnings = Vec::new();
    let mut organizations = BTreeMap::new();
    if filter.is_empty() {
        match api.get("user/orgs?per_page=100") {
            Ok(value) => for org in value.as_array().ok_or("Invalid organizations response.")? {
                let name = org["login"].as_str().ok_or("Invalid organization name.")?;
                organizations.insert(name.to_ascii_lowercase(), name.to_owned());
            },
            Err(error) => warnings.push(format!("Organization discovery: {error} CLI/OAuth membership discovery needs read:org. Organizations will still be discovered from accessible repository owners.")),
        }
    }

    let repos = api.get("user/repos?per_page=100")?;
    let mut repos: Vec<Value> = repos
        .as_array()
        .ok_or("Invalid repositories response.")?
        .iter()
        .filter(|r| {
            r["owner"]["type"] == "Organization"
                || r["owner"]["login"].as_str().is_some_and(|s| {
                    s.eq_ignore_ascii_case(login)
                        || organizations.contains_key(&s.to_ascii_lowercase())
                })
        })
        .filter(|r| {
            filter.is_empty()
                || filter.iter().any(|s| {
                    r["full_name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&qualified_repo(login, s)))
                })
        })
        .cloned()
        .collect();

    for name in filter {
        if !repos.iter().any(|r| {
            r["full_name"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(&qualified_repo(login, name)))
        }) {
            return Err(format!("Repository {name} was not found among your accessible personal or organization repositories. Check the name and token access."));
        }
    }

    for repo in &repos {
        let owner = repo["owner"]["login"]
            .as_str()
            .ok_or("Invalid repository owner.")?;
        if !owner.eq_ignore_ascii_case(login) {
            organizations.insert(owner.to_ascii_lowercase(), owner.to_owned());
        }
    }

    repos.sort_by(|a, b| a["full_name"].as_str().cmp(&b["full_name"].as_str()));
    let mut runners = Vec::new();
    let mut jobs = Vec::new();
    for org in organizations.values() {
        match api.get(&format!("orgs/{org}/actions/runners?per_page=100")) {
            Ok(value) => for runner in value.as_array().ok_or("Invalid organization runners response.")? {
                runners.push(normalize_runner(runner, "organization", org));
            },
            Err(error) => warnings.push(format!("{org} organization runners: {error} Organization inventory needs the appropriate administrator/runner role, plus admin:org for CLI/OAuth/classic tokens or Self-hosted runners: read on an organization-owned fine-grained token. Authorize organization SSO if required.")),
        }
    }

    // Load all inventory before jobs so shared pools can match every repository.
    for repo in &repos {
        let name = repo["full_name"]
            .as_str()
            .ok_or("Invalid repository name.")?;
        let owner = repo["owner"]["login"]
            .as_str()
            .ok_or("Invalid repository owner.")?;
        match api.get(&format!("repos/{name}/actions/runners?per_page=100")) {
            Ok(value) => {
                for runner in value.as_array().ok_or("Invalid runners response.")? {
                    let shared = runners.iter().any(|known| {
                        known["organization"]
                            .as_str()
                            .is_some_and(|org| org.eq_ignore_ascii_case(owner))
                            && known["id"] == runner["id"]
                            && known["name"] == runner["name"]
                    });
                    if !shared {
                        runners.push(normalize_runner(runner, "repository", name));
                    }
                }
            }
            Err(error) => warnings.push(format!("{name} runners: {error}")),
        }
    }

    for repo in &repos {
        let name = repo["full_name"]
            .as_str()
            .ok_or("Invalid repository name.")?;
        let owner = repo["owner"]["login"]
            .as_str()
            .ok_or("Invalid repository owner.")?;
        let pool: Vec<&Value> = runners
            .iter()
            .filter(|runner| {
                runner["repo"]
                    .as_str()
                    .is_some_and(|r| r.eq_ignore_ascii_case(name))
                    || runner["organization"]
                        .as_str()
                        .is_some_and(|org| org.eq_ignore_ascii_case(owner))
            })
            .collect();
        let mut seen_runs = HashSet::new();
        let mut seen_jobs = HashSet::new();
        for status in ["queued", "in_progress"] {
            let runs = match api.get(&format!(
                "repos/{name}/actions/runs?status={status}&per_page=100"
            )) {
                Ok(value) => value,
                Err(error) => {
                    warnings.push(format!("{name} {status} workflows: {error}"));
                    continue;
                }
            };
            // GitHub caps status-filtered run searches at 1,000 results.
            if runs.as_array().is_some_and(|r| r.len() >= 1000) {
                warnings.push(format!("{name}: GitHub's 1,000-run search limit was reached; this view may be incomplete."));
            }
            for run in runs.as_array().ok_or("Invalid workflow runs response.")? {
                let id = run["id"].as_u64().ok_or("Invalid workflow run ID.")?;
                if !seen_runs.insert(id) {
                    continue;
                }
                let value = match api.get(&format!(
                    "repos/{name}/actions/runs/{id}/jobs?filter=latest&per_page=100"
                )) {
                    Ok(value) => value,
                    Err(error) => {
                        warnings.push(format!("{name} run {id}: {error}"));
                        continue;
                    }
                };
                for job in value.as_array().ok_or("Invalid jobs response.")? {
                    let status = job["status"].as_str().unwrap_or("");
                    if !["queued", "in_progress", "waiting", "pending", "requested"]
                        .contains(&status)
                    {
                        continue;
                    }
                    let labels: Vec<&str> = job["labels"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect();
                    let assigned = job["runner_id"].as_u64().filter(|id| *id != 0);
                    let matching: Vec<Value> = pool
                        .iter()
                        .filter(|runner| {
                            !labels.is_empty()
                                && labels.iter().all(|label| {
                                    runner["labels"].as_array().unwrap().iter().any(|l| {
                                        l.as_str().is_some_and(|s| s.eq_ignore_ascii_case(label))
                                    })
                                })
                        })
                        .map(|r| r["key"].clone())
                        .collect();
                    let assigned_runners: Vec<_> = pool
                        .iter()
                        .filter(|r| {
                            assigned.is_some_and(|id| r["id"].as_u64() == Some(id))
                                && job["runner_name"]
                                    .as_str()
                                    .is_none_or(|name| r["name"].as_str() == Some(name))
                        })
                        .collect();
                    // Do not guess if IDs are ambiguous between distinct scopes.
                    let assigned_key = if assigned_runners.len() == 1 {
                        Some(assigned_runners[0]["key"].clone())
                    } else {
                        None
                    };
                    if assigned_runners.is_empty()
                        && matching.is_empty()
                        && !labels.iter().any(|s| s.eq_ignore_ascii_case("self-hosted"))
                    {
                        continue;
                    }
                    let job_id = job["id"].as_u64().ok_or("Invalid job ID.")?;
                    if !seen_jobs.insert(job_id) {
                        continue;
                    }
                    jobs.push(json!({
                        "id": job_id,
                        "name": job["name"],
                        "repo": name,
                        "workflow": run["name"],
                        "branch": run["head_branch"],
                        "run_number": run["run_number"],
                        "created_at": run["created_at"],
                        "status": status,
                        "runner_id": assigned,
                        "runner_name": job["runner_name"],
                        "labels": labels,
                        "matching_runner_keys": matching,
                        "assigned_runner_key": assigned_key,
                        "actions_url": format!("https://github.com/{name}/actions"),
                    }));
                }
            }
        }
    }

    jobs.sort_by(|a, b| {
        a["created_at"]
            .as_str()
            .cmp(&b["created_at"].as_str())
            .then(a["id"].as_u64().cmp(&b["id"].as_u64()))
    });

    let count = |state: &str| runners.iter().filter(|r| r["state"] == state).count();
    let running = jobs.iter().filter(|j| j["status"] == "in_progress").count();

    Ok(json!({
        "username": login,
        "repositories": repos.len(),
        "organizations": organizations.len(),
        "summary": {
            "available": count("available"),
            "busy": count("busy"),
            "offline": count("offline"),
            "queued": jobs.len() - running,
            "running": running,
        },
        "runners": runners,
        "jobs": jobs,
        "warnings": warnings,
    }))
}

fn qualified_repo(login: &str, name: &str) -> String {
    if name.contains('/') {
        name.to_owned()
    } else {
        format!("{login}/{name}")
    }
}

fn normalize_runner(runner: &Value, scope: &str, owner: &str) -> Value {
    let state = if runner["status"] != "online" {
        "offline"
    } else if runner["busy"] == true {
        "busy"
    } else {
        "available"
    };
    let labels: Vec<&str> = runner["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["name"].as_str())
        .collect();
    json!({
        "key": format!("{scope}:{}:{}", owner.to_ascii_lowercase(), runner["id"]),
        "id": runner["id"],
        "name": runner["name"],
        "os": runner["os"],
        "state": state,
        "labels": labels,
        "scope": scope,
        "organization": if scope == "organization" { Some(owner) } else { None },
        "repo": if scope == "repository" { Some(owner) } else { None },
    })
}

pub fn page_items(value: Value, key: Option<&str>) -> Result<Vec<Value>, String> {
    let pages = value
        .as_array()
        .ok_or("Invalid paginated GitHub response.")?;

    let mut items = Vec::new();
    for page in pages {
        let entries = match key {
            Some(key) => page[key].as_array(),
            None => page.as_array(),
        }
        .ok_or("Invalid GitHub response page.")?;
        items.extend(entries.iter().cloned());
    }

    Ok(items)
}
