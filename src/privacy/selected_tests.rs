use super::*;

fn selected_fixture() -> (tempfile::TempDir, PathBuf, Arc<Context>) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let scope = root.path().join("profile");
    std::fs::create_dir(&scope).unwrap();
    let policy_path = scope
        .join("privacy/projects")
        .join(digest(workspace.as_os_str().as_encoded_bytes()))
        .join("policy.json");
    let context = Context::from_registry(
        scope,
        workspace.clone(),
        policy_path,
        CurrentPolicy::new(Policy::default(), true),
        AliasRegistry::new(Uuid::new_v4()),
        None,
        true,
    )
    .unwrap();
    (root, workspace, context)
}

#[test]
fn selected_filter_activates_without_writing_policy_and_keeps_ordinary_content() {
    let (_root, _workspace, context) = selected_fixture();
    assert!(context.enabled());
    assert!(!context.policy_path.exists());
    assert!(!context.refresh_policy().unwrap());
    let source = "fn check(value: i64) -> bool { value > 0 && true }\n";
    assert_eq!(
        context.project_source("probe.rs", source).unwrap().text,
        source
    );
    for (name, source) in [
        ("unknown.ext", "arbitrary source = true; 123"),
        ("broken.py", "if bad(:"),
    ] {
        assert_eq!(context.project_source(name, source).unwrap().text, source);
    }
    let output = "running 12 tests\nASCII AUDIT 123\ntest result: ok. 12 passed\n";
    assert_eq!(context.project_retrieved(output, None).unwrap(), output);
    assert!(!context.policy_path.exists());
}

#[test]
fn selected_filter_automatic_html_native_edit_without_rules() {
    let (_root, workspace, context) = selected_fixture();
    let source = r#"<main class="card"><h1>PRIVATE_PAGE_308</h1><a href="https://private.example/308">PRIVATE_LINK_308</a><style>.card { color: red; }</style></main>"#;
    let path = workspace.join("index.html");
    std::fs::write(&path, source).unwrap();
    let projection = context.project_source("index.html", source).unwrap();
    let sent = context.mask_known(&projection.text).unwrap();
    assert!(!sent.contains("PRIVATE_PAGE_308"));
    assert!(!sent.contains("private.example"));
    assert!(sent.contains("class=\"card\"") && sent.contains("color: red"));
    assert!(!context
        .project_retrieved("PRIVATE_PAGE_308 failed; ASCII AUDIT 123", None)
        .unwrap()
        .contains("PRIVATE_PAGE_308"));
    let args = context
        .prepare_edit_ticket(
            "write_file",
            &serde_json::json!({"path":path,"content":sent.replace("color: red", "color: blue")}),
        )
        .unwrap();
    context.consume_edit_ticket("write_file", &args).unwrap();
    assert_eq!(args["content"], source.replace("color: red", "color: blue"));
    assert!(context.policy().rules.is_empty());
    assert!(!context.policy_path.exists());
}

#[test]
fn selected_filter_masks_only_chosen_identity_in_source_and_diagnostics_after_reload() {
    let (_root, workspace, context) = selected_fixture();
    let source = "def calculate(secret_customer, value):\n    return secret_customer + value + 1\n";
    std::fs::write(workspace.join("probe.py"), source).unwrap();
    let start = source.find("secret_customer").unwrap();
    let mut policy = context.policy();
    policy.rules.push(policy::Rule {
        file: "probe.py".into(),
        selector: format!(
            "/@digest/{}/{start}/{}",
            projection::digest(source),
            start + "secret_customer".len()
        ),
        action: Action::Alias,
    });
    context.save_policy(policy).unwrap();
    assert!(!context.refresh_policy().unwrap());
    let projection = context.project_source("probe.py", source).unwrap();
    let sent = context.mask_known(&projection.text).unwrap();
    assert!(!sent.contains("secret_customer"), "{sent}");
    assert!(
        sent.contains("def calculate(") && sent.contains(" + value + 1"),
        "{sent}"
    );
    assert_eq!(sent.matches("__CA_PRIVATE_").count(), 2);
    let diagnostics = context
        .project_retrieved("secret_customer: failed; ASCII AUDIT 123; 1 passed", None)
        .unwrap();
    assert!(!diagnostics.contains("secret_customer"));
    assert!(diagnostics.contains("ASCII AUDIT 123; 1 passed"));
    assert_eq!(context.policy().default_action, Action::Allow);
    assert_eq!(context.stored_policy(), context.policy());
}

#[test]
fn selected_filter_native_edit_restores_aliases_from_final_read_view() {
    let (_root, workspace, context) = selected_fixture();
    let source = "def calculate(secret_customer, value):\n    return secret_customer + value + 1\n";
    let path = workspace.join("probe.py");
    std::fs::write(&path, source).unwrap();
    let mut policy = context.policy();
    policy.rules.push(policy::Rule {
        file: "probe.py".into(),
        selector: "**/identifier/*".into(),
        action: Action::Alias,
    });
    context.save_policy(policy).unwrap();
    let projected = context.project_source("probe.py", source).unwrap();
    let sent = context.mask_known(&projected.text).unwrap();
    let args = context
        .prepare_edit_ticket(
            "write_file",
            &serde_json::json!({"path":path,"content":sent.replace(" + 1", " + 2")}),
        )
        .unwrap();
    context.consume_edit_ticket("write_file", &args).unwrap();
    assert_eq!(args["content"], source.replace(" + 1", " + 2"));
}

#[test]
fn selected_filter_old_policy_serde_and_strict_mode_remain_compatible() {
    let raw = r#"{"version":1,"enabled":true,"revision":4,"rules":[]}"#;
    let stored: Policy = serde_json::from_str(raw).unwrap();
    assert_eq!(stored.default_action, Action::Hide);
    assert!(serde_json::to_value(&stored)
        .unwrap()
        .get("default_action")
        .is_none());
    let strict = CurrentPolicy::new(stored.clone(), false);
    assert_eq!(strict.value, stored);
    let selected = CurrentPolicy::new(stored.clone(), true);
    assert_eq!(selected.stored, stored);
    assert_eq!(selected.value.default_action, Action::Allow);
}

#[test]
fn selected_filter_hides_saved_selections_before_any_native_read() {
    let (_root, workspace, context) = selected_fixture();
    let source = "secret_customer = 'private-acme-canary'\nprint(secret_customer)\n";
    std::fs::write(workspace.join("probe.py"), source).unwrap();
    let mut policy = context.policy();
    policy.rules.push(policy::Rule {
        file: "*.py".into(),
        selector: "**/string/*".into(),
        action: Action::Alias,
    });
    context.save_policy(policy).unwrap();
    let sent = context
        .project_retrieved("ASCII AUDIT 123: private-acme-canary", None)
        .unwrap();
    assert!(!sent.contains("private-acme-canary"), "{sent}");
    assert!(sent.contains("ASCII AUDIT 123"));
}

#[test]
fn selected_filter_process_only_policy_can_resume_without_a_policy_file() {
    let (_root, workspace, context) = selected_fixture();
    let id = context.session_id;
    let scope = context.scope.clone();
    let mut reference = context
        .state
        .lock()
        .unwrap()
        .registry
        .stage(&scope)
        .unwrap();
    reference.optional_policy = true;
    drop(context);
    assert!(Context::open(
        scope.clone(),
        workspace.clone(),
        id,
        Some(reference.clone())
    )
    .is_ok());
    reference.optional_policy = false;
    assert!(Context::open(scope, workspace, id, Some(reference)).is_err());
}

#[test]
fn selected_filter_masks_decoded_literals_without_changing_raw_string_meaning() {
    for (path, source, hidden) in [
        ("probe.py", "value = \"private\\x2dcanary\"\n", true),
        (
            "probe.py",
            "def value():\n    return\"private\\x2dcanary\"\n",
            true,
        ),
        (
            "probe.rs",
            "fn main() { let value = \"private\\u{2d}canary\"; }\n",
            true,
        ),
        (
            "probe.js",
            "const value = \"private\\u002dcanary\";\n",
            true,
        ),
        ("probe.json", "{\"value\":\"private\\u002dcanary\"}\n", true),
        ("probe.json", "{\"private\\u002dcanary\":1}\n", true),
        ("probe.py", "value = r\"private\\x2dcanary\"\n", false),
    ] {
        let (_root, workspace, context) = selected_fixture();
        std::fs::write(workspace.join(path), source).unwrap();
        let mut policy = context.policy();
        let start = source.find("private").unwrap();
        let end = source.rfind("canary").unwrap() + "canary".len();
        policy.rules.push(policy::Rule {
            file: path.into(),
            selector: format!("/@digest/{}/{start}/{end}", projection::digest(source)),
            action: Action::Alias,
        });
        context.save_policy(policy).unwrap();
        let sent = context
            .project_retrieved("ASCII AUDIT 123: private-canary", None)
            .unwrap();
        assert_eq!(!sent.contains("private-canary"), hidden, "{path}: {sent}");
        assert!(sent.contains("ASCII AUDIT 123"));
        let projected = context.project_source(path, source).unwrap();
        let arguments = context
            .prepare_edit_ticket(
                "write_file",
                &serde_json::json!({"path":workspace.join(path),"content":projected.text}),
            )
            .unwrap();
        assert_eq!(arguments["content"], source);
    }
}

#[test]
fn selected_filter_masks_selected_paths_without_granting_unseen_edit_views() {
    let (_root, workspace, context) = selected_fixture();
    let path = workspace.join("private-customer.py");
    std::fs::write(&path, "value = 123\n").unwrap();
    let mut policy = context.policy();
    policy.rules.push(policy::Rule {
        file: "private-customer.py".into(),
        selector: "/@path".into(),
        action: Action::Hide,
    });
    context.save_policy(policy).unwrap();
    let output = format!("ASCII AUDIT 123\nprivate-customer.py\n{}\n", path.display());
    let sent = context.project_retrieved(&output, None).unwrap();
    assert!(!sent.contains("private-customer.py"), "{sent}");
    assert!(sent.contains("ASCII AUDIT 123"));
    assert!(!context
        .state
        .lock()
        .unwrap()
        .views
        .contains_key("private-customer.py"));
    assert!(context
        .prepare_edit_ticket(
            "write_file",
            &serde_json::json!({"path":path,"content":"value = 456\n"})
        )
        .is_err());
    context
        .project_source("private-customer.py", "value = 123\n")
        .unwrap();
    assert!(context
        .prepare_edit_ticket(
            "write_file",
            &serde_json::json!({"path":path,"content":"value = 456\n"})
        )
        .is_ok());
}

#[test]
fn startup_overlay_does_not_become_the_stored_project_policy() {
    let stored = Policy {
        enabled: false,
        default_action: Action::Hide,
        rules: vec![policy::Rule {
            file: "notes.md".into(),
            selector: "/paragraph/0".into(),
            action: Action::Hide,
        }],
        ..Policy::default()
    };
    let selected = overlay_policy(stored.clone(), true, false);
    assert!(selected.enabled);
    assert_eq!(selected.default_action, Action::Allow);
    assert_eq!(selected.rules, stored.rules);
    assert_ne!(selected, stored);
    let masked = overlay_policy(stored.clone(), false, true);
    assert!(masked.enabled);
    assert!(masked.rules.is_empty());
    let note = startup_override_note(&stored, true, false).expect("override is visible");
    assert!(note.contains("project policy only"));
    assert!(note.contains("Selected values"));
    assert_eq!(overlay_policy(stored.clone(), false, false), stored);
}
