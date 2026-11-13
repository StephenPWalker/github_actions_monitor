use github_actions_monitor::gh::{decode_response, failure};
use serde_json::json;

#[test]
fn decodes_paginated_organization_lists_and_runner_pools() {
    assert_eq!(
        decode_response(
            "user/orgs?per_page=100",
            json!([[{"login":"octo-team"}],[{"login":"another-team"}]])
        )
        .unwrap(),
        json!([{"login":"octo-team"},{"login":"another-team"}])
    );
    assert_eq!(
        decode_response(
            "user/repos?per_page=100",
            json!([[{"full_name":"octo-team/one"}]])
        )
        .unwrap(),
        json!([{"full_name":"octo-team/one"}])
    );
    assert_eq!(
        decode_response(
            "orgs/octo-team/actions/runners?per_page=100",
            json!([{"runners":[{"id":1}]},{"runners":[{"id":2}]}])
        )
        .unwrap(),
        json!([{"id":1},{"id":2}])
    );
}

#[test]
fn diagnostic_errors_never_echo_secrets() {
    for diagnostic in [
        "HTTP 401 token=private-value",
        "HTTP 403 private-value",
        "network private-value",
        "rate limit private-value",
    ] {
        assert!(!failure(diagnostic).contains("private-value"));
    }
}

#[test]
fn organization_role_errors_distinguish_account_permission_from_token_scopes() {
    let error = failure("gh: You must be an org admin or have the runners and runner groups fine-grained permission. HTTP 403 private-value");
    assert!(error.contains("organization administrator"));
    assert!(error.contains("runner-group permission"));
    assert!(!error.contains("private-value"));
}
