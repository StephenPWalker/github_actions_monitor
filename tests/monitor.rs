use github_actions_monitor::{connection, page_items, snapshot, Api};
use serde_json::{json, Value};
use std::collections::HashMap;

struct Fake(HashMap<String, Value>);
impl Api for Fake {
    fn get(&self, endpoint: &str) -> Result<Value, String> {
        self.0
            .get(endpoint)
            .cloned()
            .ok_or_else(|| "GitHub denied access (HTTP 403).".into())
    }
}
fn fixture() -> Fake {
    Fake(HashMap::from([
        ("user".into(), json!({"login":"octocat"})),
        ("user/orgs?per_page=100".into(), json!([])),
        (
            "user/repos?per_page=100".into(),
            json!([
                {"full_name":"octocat/alpha","owner":{"login":"octocat"}},
                {"full_name":"octocat/beta","owner":{"login":"octocat"}},
                {"full_name":"other/shared","owner":{"login":"other"}}
            ]),
        ),
        (
            "repos/octocat/alpha/actions/runners?per_page=100".into(),
            json!([
                {"id":1,"name":"idle","status":"online","busy":false,"labels":[{"name":"self-hosted"},{"name":"Linux"}]},
                {"id":2,"name":"working","status":"online","busy":true,"labels":[{"name":"self-hosted"},{"name":"Linux"}]},
                {"id":3,"name":"offline","status":"offline","busy":false,"labels":[{"name":"self-hosted"},{"name":"Windows"}]}
            ]),
        ),
        (
            "repos/octocat/alpha/actions/runs?status=queued&per_page=100".into(),
            json!([{"id":10,"name":"CI","head_branch":"main","run_number":5,"created_at":"2026-11-13T10:00:00Z"}]),
        ),
        (
            "repos/octocat/alpha/actions/runs?status=in_progress&per_page=100".into(),
            json!([{"id":10,"name":"CI","head_branch":"main","run_number":5}]),
        ),
        (
            "repos/octocat/alpha/actions/runs/10/jobs?filter=latest&per_page=100".into(),
            json!([
                {"id":11,"name":"test","status":"queued","labels":["self-hosted","linux"],"runner_id":null},
                {"id":12,"name":"build","status":"in_progress","labels":["self-hosted","Linux"],"runner_id":2},
                {"id":13,"name":"hosted","status":"queued","labels":["ubuntu-latest"]},
                {"id":14,"name":"done","status":"completed","labels":["self-hosted"]},
                {"id":15,"name":"gpu","status":"queued","labels":["self-hosted","gpu"]},
                {"id":16,"name":"custom","status":"queued","labels":["Linux"]}
            ]),
        ),
        (
            "repos/octocat/beta/actions/runners?per_page=100".into(),
            json!([]),
        ),
        (
            "repos/octocat/beta/actions/runs?status=queued&per_page=100".into(),
            json!([]),
        ),
        (
            "repos/octocat/beta/actions/runs?status=in_progress&per_page=100".into(),
            json!([]),
        ),
    ]))
}

#[test]
fn validates_inputs_before_calling_cli() {
    assert!(connection(&json!({"username":"../bad"})).is_err());
    assert!(connection(&json!({"username":"octocat","repos":"../secrets"})).is_err());
    assert!(connection(&json!({"username":"octocat","token":"bad\nvalue"})).is_err());
    let (name, token, repos) =
        connection(&json!({"username":"octocat","repos":"alpha, beta,alpha"})).unwrap();
    assert_eq!(name, "octocat");
    assert!(token.is_empty());
    assert_eq!(repos, ["alpha", "beta"]);
}

#[test]
fn paginated_objects_and_arrays_are_flattened_without_losing_items() {
    assert_eq!(
        page_items(
            json!([{ "runners":[{"id":1}] },{"runners":[{"id":2}]}]),
            Some("runners")
        )
        .unwrap()
        .len(),
        2
    );
    assert_eq!(
        page_items(json!([[1, 2], [3]]), None).unwrap(),
        vec![json!(1), json!(2), json!(3)]
    );
    assert!(page_items(json!([{"unexpected":[]}]), Some("jobs")).is_err());
}

#[test]
fn rejects_account_mismatch() {
    assert!(snapshot(&fixture(), "someone", &[])
        .unwrap_err()
        .contains("account"));
}

#[test]
fn reports_idle_busy_offline_and_only_personal_repos() {
    let s = snapshot(&fixture(), "OCTOCAT", &[]).unwrap();
    assert_eq!(s["repositories"], 2);
    assert_eq!(
        s["summary"],
        json!({"available":1,"busy":1,"offline":1,"queued":3,"running":1})
    );
    assert_eq!(s["runners"][0]["state"], "available");
}

#[test]
fn matches_all_labels_case_insensitively_and_does_not_claim_assignment() {
    let s = snapshot(&fixture(), "octocat", &[]).unwrap();
    let job = s["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == 11)
        .unwrap();
    assert_eq!(
        job["matching_runner_keys"],
        json!(["repository:octocat/alpha:1", "repository:octocat/alpha:2"])
    );
    assert!(job["runner_id"].is_null());
    assert_eq!(
        job["actions_url"],
        "https://github.com/octocat/alpha/actions"
    );
}

#[test]
fn includes_unmatched_self_hosted_and_custom_labels_but_excludes_hosted_and_completed() {
    let s = snapshot(&fixture(), "octocat", &[]).unwrap();
    let jobs = s["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 4); // The duplicated workflow was fetched only once.
    assert!(jobs
        .iter()
        .any(|j| j["id"] == 15 && j["matching_runner_keys"] == json!([])));
    assert!(jobs.iter().any(|j| j["id"] == 16));
    assert!(!jobs.iter().any(|j| j["id"] == 13 || j["id"] == 14));
}

#[test]
fn explicit_filter_is_owned_and_missing_repos_fail_clearly() {
    let s = snapshot(&fixture(), "octocat", &["beta".into()]).unwrap();
    assert_eq!(s["repositories"], 1);
    assert_eq!(s["runners"], json!([]));
    assert!(snapshot(&fixture(), "octocat", &["missing".into()]).is_err());
}

#[test]
fn partial_failures_are_visible_and_do_not_erase_other_repos() {
    let mut api = fixture();
    api.0
        .remove("repos/octocat/beta/actions/runners?per_page=100");
    let s = snapshot(&api, "octocat", &[]).unwrap();
    assert_eq!(s["runners"].as_array().unwrap().len(), 3);
    assert_eq!(s["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn labels_never_match_runners_in_another_repository() {
    let mut api = fixture();
    api.0.insert(
        "repos/octocat/beta/actions/runs?status=queued&per_page=100".into(),
        json!([{"id":20}]),
    );
    api.0.insert(
        "repos/octocat/beta/actions/runs/20/jobs?filter=latest&per_page=100".into(),
        json!([{"id":21,"status":"queued","labels":["self-hosted","Linux"]}]),
    );
    let s = snapshot(&api, "octocat", &[]).unwrap();
    let job = s["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == 21)
        .unwrap();
    assert_eq!(job["matching_runner_keys"], json!([]));
}

fn organization_fixture() -> Fake {
    let mut api = fixture();
    api.0.insert(
        "user/orgs?per_page=100".into(),
        json!([{"login":"octo-team"}]),
    );
    api.0.get_mut("user/repos?per_page=100").unwrap().as_array_mut().unwrap().extend([
        json!({"full_name":"octo-team/one","owner":{"login":"octo-team","type":"Organization"}}),
        json!({"full_name":"octo-team/two","owner":{"login":"octo-team","type":"Organization"}}),
    ]);
    api.0.insert("orgs/octo-team/actions/runners?per_page=100".into(),json!([
        {"id":50,"name":"shared-arm","status":"online","busy":false,"labels":[{"name":"self-hosted"},{"name":"ARM64"}]}
    ]));
    for (name, run_id, job_id) in [("one", 30, 31), ("two", 40, 41)] {
        api.0.insert(
            format!("repos/octo-team/{name}/actions/runners?per_page=100"),
            json!([]),
        );
        api.0.insert(
            format!("repos/octo-team/{name}/actions/runs?status=queued&per_page=100"),
            json!([{"id":run_id}]),
        );
        api.0.insert(
            format!("repos/octo-team/{name}/actions/runs?status=in_progress&per_page=100"),
            json!([]),
        );
        api.0.insert(
            format!("repos/octo-team/{name}/actions/runs/{run_id}/jobs?filter=latest&per_page=100"),
            json!([
                {"id":job_id,"name":"test-arm","status":"queued","labels":["self-hosted","arm64"]}
            ]),
        );
    }
    api
}

#[test]
fn shared_organization_runner_is_counted_once_and_matches_jobs_across_its_repos() {
    let s = snapshot(&organization_fixture(), "octocat", &[]).unwrap();
    assert_eq!(s["repositories"], 4);
    assert_eq!(s["organizations"], 1);
    assert_eq!(s["summary"]["available"], 2);
    let runners = s["runners"].as_array().unwrap();
    assert_eq!(
        runners.iter().filter(|r| r["name"] == "shared-arm").count(),
        1
    );
    let runner = runners.iter().find(|r| r["name"] == "shared-arm").unwrap();
    assert_eq!(runner["organization"], "octo-team");
    assert_eq!(runner["scope"], "organization");
    for id in [31, 41] {
        let job = s["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == id)
            .unwrap();
        assert_eq!(
            job["matching_runner_keys"],
            json!(["organization:octo-team:50"])
        );
    }
}

#[test]
fn organization_inventory_is_visible_even_without_repositories_or_jobs() {
    let mut api = organization_fixture();
    api.0.insert("user/repos?per_page=100".into(), json!([]));
    let s = snapshot(&api, "octocat", &[]).unwrap();
    assert_eq!(s["repositories"], 0);
    assert_eq!(s["runners"].as_array().unwrap().len(), 1);
    assert_eq!(s["summary"]["available"], 1);
}

#[test]
fn fine_grained_tokens_discover_organizations_from_repository_owners() {
    let mut api = organization_fixture();
    // GitHub returns an empty memberships list for fine-grained PATs.
    api.0.insert("user/orgs?per_page=100".into(), json!([]));
    let s = snapshot(&api, "octocat", &[]).unwrap();
    assert_eq!(s["organizations"], 1);
    assert!(s["runners"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["name"] == "shared-arm"));
}

#[test]
fn qualified_repository_filter_selects_organization_scope() {
    let (_, _, repos) =
        connection(&json!({"username":"octocat","repos":"octo-team/one, beta"})).unwrap();
    assert_eq!(repos, ["octo-team/one", "beta"]);
    for invalid in [
        "octo-team/../one",
        "/one",
        "octo-team/",
        "https://github.com/octo-team/one",
    ] {
        assert!(connection(&json!({"username":"octocat","repos":invalid})).is_err());
    }
    let s = snapshot(
        &organization_fixture(),
        "octocat",
        &["OCTO-TEAM/ONE".into()],
    )
    .unwrap();
    assert_eq!(s["repositories"], 1);
    assert_eq!(s["runners"].as_array().unwrap().len(), 1);
    assert_eq!(s["jobs"].as_array().unwrap().len(), 1);
    let personal = snapshot(&organization_fixture(), "octocat", &["beta".into()]).unwrap();
    assert_eq!(personal["organizations"], 0);
    assert_eq!(personal["runners"], json!([]));
}

#[test]
fn organization_permission_failures_warn_without_erasing_personal_runners() {
    let mut api = organization_fixture();
    api.0.remove("orgs/octo-team/actions/runners?per_page=100");
    let s = snapshot(&api, "octocat", &[]).unwrap();
    assert_eq!(s["runners"].as_array().unwrap().len(), 3);
    let warnings = s["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w
        .as_str()
        .unwrap()
        .contains("octo-team organization runners")));
    assert!(warnings
        .iter()
        .any(|w| w.as_str().unwrap().contains("admin:org")));
}

#[test]
fn failed_membership_discovery_still_uses_organization_repository_metadata() {
    let mut api = organization_fixture();
    api.0.remove("user/orgs?per_page=100");
    let s = snapshot(&api, "octocat", &[]).unwrap();
    assert_eq!(s["organizations"], 1);
    assert_eq!(s["runners"].as_array().unwrap().len(), 4);
    assert!(!s["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn repeated_organization_runner_in_repository_response_is_not_counted_twice() {
    let mut api = organization_fixture();
    let shared = api.0["orgs/octo-team/actions/runners?per_page=100"].clone();
    api.0.insert(
        "repos/octo-team/one/actions/runners?per_page=100".into(),
        shared,
    );
    let s = snapshot(&api, "octocat", &[]).unwrap();
    assert_eq!(s["runners"].as_array().unwrap().len(), 4);
    assert_eq!(s["summary"]["available"], 2);
}

#[test]
fn running_jobs_are_associated_with_the_organization_runner() {
    let mut api = organization_fixture();
    api.0.insert("repos/octo-team/one/actions/runs/30/jobs?filter=latest&per_page=100".into(),json!([
        {"id":31,"name":"running","status":"in_progress","runner_id":50,"runner_name":"shared-arm","labels":["self-hosted","ARM64"]}
    ]));
    let s = snapshot(&api, "octocat", &[]).unwrap();
    let job = s["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == 31)
        .unwrap();
    assert_eq!(job["assigned_runner_key"], "organization:octo-team:50");
}

#[test]
fn organization_runner_ids_and_labels_are_isolated_from_personal_repositories() {
    let mut api = organization_fixture();
    api.0
        .get_mut("orgs/octo-team/actions/runners?per_page=100")
        .unwrap()[0]["id"] = json!(2);
    let s = snapshot(&api, "octocat", &[]).unwrap();
    let personal = s["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == 12)
        .unwrap();
    assert_eq!(
        personal["assigned_runner_key"],
        "repository:octocat/alpha:2"
    );
    let org = s["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == 31)
        .unwrap();
    assert_eq!(
        org["matching_runner_keys"],
        json!(["organization:octo-team:2"])
    );
}
