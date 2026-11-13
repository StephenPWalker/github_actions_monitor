use std::process::Command;

#[test]
fn active_dependencies_offer_mit_without_additional_license_obligations() {
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--locked",
            "--offline",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
            "--format",
            "{p}|{l}",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Could not inspect dependency licenses with cargo tree");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8(output.stdout).expect("Dependency licenses must be UTF-8");
    assert!(!tree.trim().is_empty(), "Dependency tree must not be empty");
    for line in tree.lines() {
        let (package, expression) = line.split_once('|').expect("Missing license metadata");
        let expression = expression.trim_end_matches(" (*)").trim();
        let choices: Vec<_> = expression.split(" OR ").collect();
        // Accept a plain MIT license or an OR choice containing MIT. Reject
        // compound AND obligations, missing metadata, and other expressions.
        assert!(
            choices.contains(&"MIT")
                && choices.iter().all(|choice| {
                    !choice.is_empty()
                        && choice
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
                }),
            "{package} has no MIT-only licensing option: {expression}"
        );
    }
}
