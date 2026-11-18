//! The provider filter and project-privacy rewriter no longer change a request.

#[test]
fn removed_filter_shell_keeps_workspace_path() {
    let command = "ls /Volumes/projects/ca";
    let restored = crate::privacy::shell::restore_filename_references(command, |_| {
        Err("private reference".into())
    })
    .unwrap();
    assert_eq!(restored, command);
}

#[test]
fn removed_filter_listing_keeps_real_filename() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("project");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("real-name.txt"), "hello").unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let context = crate::privacy::Context::open(
        temp.path().join("profile"),
        workspace.clone(),
        uuid::Uuid::new_v4(),
        None,
    )
    .unwrap();
    assert!(!context.enabled());
    assert!(!context.filenames_enabled());
    let shown = context
        .project_filename_path(&workspace.join("real-name.txt"))
        .unwrap();
    assert!(shown.ends_with("real-name.txt"), "{shown}");
    assert_eq!(context.mask_known(&shown).unwrap(), shown);
    let names = std::fs::read_dir(&workspace)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(names.contains(&"real-name.txt".to_string()), "{names:?}");
}

#[test]
fn removed_filter_stop_phrase_passes() {
    let config: crate::provider_filter::ProviderFilterConfig = toml::from_str(
        "enabled = true\nphrases = [\"stop-phrase-sample\"]\n[hide]\nfilenames = true\n",
    )
    .unwrap();
    let filter = crate::provider_filter::ProviderFilter::compile(&config).unwrap();
    let body = br#"{"text":"stop-phrase-sample"}"#;
    assert_eq!(filter.prepare_request(body).unwrap(), body);
    assert!(!filter.is_tripped());
    assert!(crate::provider_filter::scan_outbound(body).is_ok());
    assert!(!crate::provider_filter::is_tripped());
}

#[test]
fn removed_filter_enabled_config_does_not_rewrite() {
    let config: crate::provider_filter::ProviderFilterConfig =
        toml::from_str("enabled = true\n[hide]\nfilenames = true\n").unwrap();
    assert!(config.enabled);
    assert!(config.hide.filenames);
    let filter = crate::provider_filter::ProviderFilter::compile(&config).unwrap();
    assert!(!filter.filenames());
    let body = b"src/main.rs";
    assert_eq!(filter.prepare_request(body).unwrap(), body);
    assert!(!filter.is_tripped());
}

#[test]
fn removed_filter_transcript_has_no_status_row() {
    let mut state = crate::tui::test_support::test_state();
    state.task_filter_preview_enabled = true;
    *state.filter_presentation.lock().unwrap() = crate::provider_filter::FilterPresentation {
        status: crate::provider_filter::FilterStatus::Active,
        detail: "Project privacy · Selected values only · filenames on · 0 selected values".into(),
    };
    let screen = crate::tui::test_support::render_to_text(&state, 100, 24);
    assert!(!screen.contains("Project privacy"), "{screen}");
    assert!(!screen.contains("Filter off"), "{screen}");
    assert!(!screen.contains("Selected values"), "{screen}");
    assert!(!screen.contains("filenames on"), "{screen}");
    assert!(!screen.contains("FILTER ON"), "{screen}");
    assert!(!screen.contains("TO PROVIDER"), "{screen}");
}
