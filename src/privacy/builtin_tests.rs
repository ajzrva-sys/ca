use super::*;
use serde_json::json;

fn fixture() -> (tempfile::TempDir, PathBuf, Arc<Context>) {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("project");
    let scope = temp.path().join("profile");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&scope).unwrap();
    std::fs::write(
        workspace.join("PrivatePage316.html"),
        "<p>private text</p>\n",
    )
    .unwrap();
    let context = Context::from_registry(
        scope.clone(),
        workspace.clone(),
        scope.join("privacy/policy.json"),
        CurrentPolicy::new(Policy::default(), true),
        AliasRegistry::new(Uuid::new_v4()),
        None,
        true,
    )
    .unwrap();
    context.refresh_filenames().unwrap();
    (temp, workspace, context)
}
fn projected_listing(context: &Context, directory: &str, listing: &str) -> Result<String, String> {
    use crate::agent::client::{ChatMessage, FunctionCall, ToolCallMsg};
    use crate::image_content::ModelContent;
    let messages = vec![
        ChatMessage {
            role: "assistant".into(),
            content: ModelContent::text("List artifacts"),
            tool_calls: Some(vec![ToolCallMsg {
                id: "artifact-list".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "list_dir".into(),
                    arguments: json!({"path":directory}).to_string(),
                    arguments_retrieved_spans: vec![],
                },
            }]),
            tool_call_id: None,
            response_items: vec![],
        },
        ChatMessage {
            role: "tool".into(),
            content: ModelContent::text(listing),
            tool_calls: None,
            tool_call_id: Some("artifact-list".into()),
            response_items: vec![],
        },
    ];
    let mut request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        "privacy-test",
        "model",
        messages,
        vec![],
        Default::default(),
    );
    context.prepare_request(&mut request)?;
    Ok(request.messages[1].content.as_str().unwrap().to_owned())
}

fn configured(run: impl FnOnce()) {
    let settings = crate::provider_filter::ProviderFilterConfig {
        enabled: true,
        hide: crate::provider_filter::FilterHide {
            filenames: true,
            ..Default::default()
        },
        ..Default::default()
    };
    crate::provider_filter::with_test_filter(&settings, run);
}

#[test]
fn builtin_contract_admission_preserves_all_profiles_and_custom_ceiling() {
    configured(|| {
        let (_temp, _workspace, context) = fixture();
        for spec in crate::agent::registry::ToolRegistry::builtins().specs() {
            context.check_filename_tool(spec.name.as_str()).unwrap();
        }
        assert!(context.check_filename_tool("mcp__private_custom").is_err());
    });
}

#[test]
fn builtin_local_payloads_restore_paths_without_rewriting_schema_controls() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        let path = context
            .project_filename_path(&workspace.join("PrivatePage316.html"))
            .unwrap();
        for name in [
            "read_pdf",
            "read_lints",
            "read_todos",
            "blame",
            "lsp_symbols",
            "lsp_definition",
            "lsp_references",
            "lsp_hover",
            "lsp_implementation",
            "lsp_call_hierarchy",
        ] {
            let restored = context
                .prepare_edit_ticket(name, &json!({"path":path}))
                .unwrap();
            assert_eq!(
                restored["path"],
                workspace.join("PrivatePage316.html").to_str().unwrap(),
                "{name}"
            );
        }
        let control = json!({"output_schema":{"type":"object","properties":{path.clone():{"type":"string","enum":[path.clone()]}}},"route":{"provider":path},"prompt":format!("Inspect {path}")});
        let restored = context
            .prepare_edit_ticket("spawn_agent", &control)
            .unwrap();
        assert_eq!(restored["output_schema"], control["output_schema"]);
        assert_eq!(restored["route"], control["route"]);
        assert!(restored["prompt"]
            .as_str()
            .unwrap()
            .contains("PrivatePage316.html"));
        let image = context
            .prepare_edit_ticket(
                "generate_image",
                &json!({"prompt":format!("Render {path}"),"image_url":path}),
            )
            .unwrap();
        assert!(!image["prompt"]
            .as_str()
            .unwrap()
            .contains("PrivatePage316.html"));
        assert_eq!(
            image["image_url"],
            workspace.join("PrivatePage316.html").to_str().unwrap()
        );
    });
}

#[test]
fn builtin_repair_checks_restore_typed_argv_and_exact_command_grants() {
    configured(|| {
        let (temp, _workspace, context) = fixture();
        let workspace = context.workspace().to_path_buf();
        let external = temp.path().join("PrivateRuntime317");
        std::fs::create_dir(&external).unwrap();
        let executable = external.join("PrivatePython317");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let executable = std::fs::canonicalize(executable).unwrap();
        let checker = workspace.join("PrivateChecker317.py");
        let target = workspace.join("PrivateTarget317.py");
        std::fs::write(&checker, "raise SystemExit(0)\n").unwrap();
        let source = "value = 0\n";
        std::fs::write(&target, source).unwrap();
        let mut authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
        authority
            .add_root(executable.parent().unwrap(), false)
            .unwrap();
        context.authorize_tool_roots(&authority).unwrap();

        let executable_alias = context.project_filename_path(&executable).unwrap();
        let checker_alias = context.project_filename_path(&checker).unwrap();
        let target_alias = context.project_filename_path(&target).unwrap();
        let cwd_alias = context.project_filename_path(&workspace).unwrap();
        let argv = vec![executable_alias.clone(), checker_alias];
        let native_argv = vec![
            executable.to_str().unwrap().to_owned(),
            checker.to_str().unwrap().to_owned(),
        ];
        let arguments = json!({
            "task":"Repair the frozen target", "tools":["read_file","edit_file","run_command"],
            "commands":[crate::narrow_worker::render_command(&argv)],
            "repair_contract":{
                "path":target_alias, "source_sha256":crate::narrow_worker::digest(source),
                "start_line":1, "end_line":1, "context":"Keep the fixed check",
                "checks":[{"argv":argv, "cwd":cwd_alias, "timeout_secs":1}],
                "max_attempts":1
            }
        });
        for name in ["spawn_agent", "spawn_agent_batch"] {
            let wire = if name == "spawn_agent_batch" {
                json!({"tasks":[arguments.clone()]})
            } else {
                arguments.clone()
            };
            let restored = context.prepare_edit_ticket(name, &wire).unwrap();
            let restored = if name == "spawn_agent_batch" {
                &restored["tasks"][0]
            } else {
                &restored
            };
            assert_eq!(
                restored["repair_contract"]["path"],
                target.to_str().unwrap()
            );
            assert_eq!(
                restored["repair_contract"]["checks"][0]["argv"],
                json!(native_argv)
            );
            assert_eq!(
                restored["repair_contract"]["checks"][0]["cwd"],
                workspace.to_str().unwrap()
            );
            assert_eq!(
                restored["commands"],
                json!([crate::narrow_worker::render_command(&native_argv)])
            );
        }

        let mut mismatched = arguments.clone();
        mismatched["commands"][0] = json!(format!(
            "{} --different-grant",
            crate::narrow_worker::render_command(&argv)
        ));
        assert!(context
            .prepare_edit_ticket("spawn_agent", &mismatched)
            .is_err());

        let mut unknown = arguments.clone();
        let unknown_argv = vec!["not-a-real-path/f999".to_owned(), argv[1].clone()];
        unknown["repair_contract"]["checks"][0]["argv"] = json!(unknown_argv);
        unknown["commands"][0] = json!(crate::narrow_worker::render_command(&unknown_argv));
        assert!(context
            .prepare_edit_ticket("spawn_agent", &unknown)
            .is_err());

        // Typed repair checks do not make arbitrary executable shell aliases safe.
        assert!(context
            .prepare_edit_ticket(
                "run_command",
                &json!({
                    "command":crate::narrow_worker::render_command(&argv), "cwd":cwd_alias
                })
            )
            .is_err());

        context
            .authorize_tool_roots(
                &crate::workspace::WorkspaceContext::discover(&workspace).unwrap(),
            )
            .unwrap();
        assert!(context
            .prepare_edit_ticket("spawn_agent", &arguments)
            .is_err());
    });
}

#[test]
fn builtin_nested_creation_and_external_grants_are_live_not_snapshot_authority() {
    configured(|| {
        let (temp, workspace, context) = fixture();
        let external = temp.path().join("PrivateExport316");
        std::fs::create_dir(&external).unwrap();
        std::fs::write(external.join("PrivateAsset316.svg"), "<svg/>").unwrap();
        let mut authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
        authority.add_root(&external, false).unwrap();
        context.authorize_tool_roots(&authority).unwrap();
        let alias = context.project_filename_path(&external).unwrap();
        context.allow_ephemeral();
        let listing = projected_listing(&context, &alias, "PrivateAsset316.svg").unwrap();
        assert!(!listing.contains("PrivateAsset316.svg"));
        assert_eq!(
            context.resolve_filename_path(&listing, false).unwrap(),
            external.join("PrivateAsset316.svg")
        );
        let args = context
            .prepare_edit_ticket(
                "create_directory",
                &json!({"path":format!("{alias}/site/styles")}),
            )
            .unwrap();
        assert_eq!(args["path"], external.join("site/styles").to_str().unwrap());
        let snapshot = context
            .state
            .lock()
            .unwrap()
            .filenames
            .as_ref()
            .unwrap()
            .snapshot();
        let mut loaded =
            filenames::FilenameAliases::from_snapshot(workspace.clone(), snapshot).unwrap();
        assert!(loaded.resolve_path(&alias).is_err());
        loaded
            .authorize_roots(std::slice::from_ref(&external))
            .unwrap();
        assert_eq!(
            loaded.resolve_path(&alias).unwrap(),
            external.canonicalize().unwrap()
        );
        loaded.authorize_roots(&[]).unwrap();
        assert!(loaded.resolve_path(&alias).is_err());
        context
            .authorize_tool_roots(
                &crate::workspace::WorkspaceContext::discover(&workspace).unwrap(),
            )
            .unwrap();
        assert!(context
            .prepare_edit_ticket("read_file", &json!({"path":alias}))
            .is_err());
        assert!(projected_listing(&context, &alias, "PrivateAsset316.svg").is_err());
    });
}

#[test]
fn builtin_command_generations_reuse_artifacts_and_isolate_original_source() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        let authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
        context.authorize_tool_roots(&authority).unwrap();
        let alias = context
            .project_filename_path(&workspace.join("PrivatePage316.html"))
            .unwrap();
        let args = context
            .prepare_edit_ticket(
                "run_command",
                &json!({"command":format!("cat '{alias}'"),"cwd":"."}),
            )
            .unwrap();
        let first = context
            .prepare_command_workspace(
                args["command"].as_str().unwrap(),
                workspace.to_str().unwrap(),
                std::slice::from_ref(&workspace),
            )
            .unwrap();
        assert!(first.command.contains(&first.cwd));
        assert!(!first.command.contains(workspace.to_str().unwrap()));
        let generated = Path::new(&first.cwd).join("result.txt");
        std::fs::write(&generated, "build result").unwrap();
        std::fs::write(
            Path::new(&first.cwd).join("PrivatePage316.html"),
            "changed clone",
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(workspace.join("PrivatePage316.html")).unwrap(),
            "<p>private text</p>\n"
        );
        let second = context
            .prepare_command_workspace(
                "true",
                workspace.to_str().unwrap(),
                std::slice::from_ref(&workspace),
            )
            .unwrap();
        assert_eq!(first.cwd, second.cwd);
        assert!(generated.exists());
        context.refresh_filenames().unwrap();
        assert!(!context
            .project_filename_path(&generated)
            .unwrap()
            .contains("result.txt"));
        std::fs::write(workspace.join("PrivatePage316.html"), "native edit").unwrap();
        let third = context
            .prepare_command_workspace(
                "true",
                workspace.to_str().unwrap(),
                std::slice::from_ref(&workspace),
            )
            .unwrap();
        assert_ne!(second.generation, third.generation);
        assert!(generated.exists());
    });
}

#[test]
fn builtin_shell_references_cannot_become_evaluated_code_or_stdin() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        let alias = context
            .project_filename_path(&workspace.join("PrivatePage316.html"))
            .unwrap();
        for command in [
            alias.clone(),
            format!("eval '{alias}'"),
            format!("echo $({alias})"),
            format!("echo \"${{x:-{alias}}}\""),
        ] {
            assert!(context
                .prepare_edit_ticket("run_command", &json!({"command":command}))
                .is_err());
        }
        assert!(context
            .prepare_edit_ticket(
                "write_shell_stdin",
                &json!({"job_id":"opaque","input":alias})
            )
            .is_err());
        let root = context.project_filename_path(&workspace).unwrap();
        assert!(context
            .prepare_edit_ticket(
                "run_command",
                &json!({"command":format!("printf x > '{root}/../escape.txt'")})
            )
            .is_err());
        std::fs::remove_file(workspace.join("PrivatePage316.html")).unwrap();
        context.refresh_filenames().unwrap();
        assert!(
            context
                .prepare_edit_ticket(
                    "run_command",
                    &json!({"command":format!("printf x > '{alias}'")})
                )
                .is_err(),
            "creation must not revive a deleted private reference"
        );
    });
}

#[test]
fn builtin_projected_batch_predicates_select_exact_hidden_files() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        std::fs::write(workspace.join("OtherPrivate316.html"), "<p>other</p>").unwrap();
        let authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
        context.authorize_tool_roots(&authority).unwrap();
        context.refresh_filenames().unwrap();
        let reference = context
            .project_filename_path(&workspace.join("PrivatePage316.html"))
            .unwrap();
        let args = context
            .prepare_edit_ticket(
                "select_files",
                &json!({"folder":".","criteria":{"name_glob":reference}}),
            )
            .unwrap();
        let selected = tasks::call_for(&context, "select", args).unwrap();
        assert_eq!(selected["count"], 1);
        let args = context
            .prepare_edit_ticket("select_files", &json!({"folder":reference}))
            .unwrap();
        let selected = tasks::call_for(&context, "select", args).unwrap();
        assert_eq!(selected["count"], 1);
    });
}

#[test]
fn builtin_url_fields_use_uri_paths_and_image_payloads_stay_private() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        let reference = context
            .project_filename_path(&workspace.join("PrivatePage316.html"))
            .unwrap();
        for name in ["browser", "web_fetch", "fetch"] {
            let url = format!("http://127.0.0.1:8123/{reference}?mode=2#top");
            let args = context
                .prepare_edit_ticket(name, &json!({"url":url}))
                .unwrap();
            assert_eq!(
                args["url"], "http://127.0.0.1:8123/PrivatePage316.html?mode=2#top",
                "{name}"
            );
        }
        let args = context.prepare_edit_ticket("generate_image", &json!({"prompt":format!("Render {reference}"),"filename":"public-image316.png","image_url":"https://example.invalid/reference.png"})).unwrap();
        assert_eq!(
            args["filename"],
            workspace
                .join(".ca-images/public-image316.png")
                .to_str()
                .unwrap()
        );
        assert_eq!(args["image_url"], "https://example.invalid/reference.png");
        assert!(!args["prompt"]
            .as_str()
            .unwrap()
            .contains("PrivatePage316.html"));
        let authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
        crate::workspace::prepare_tool_invocation(
            &authority,
            "generate_image",
            &args,
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
    });
}

#[test]
fn builtin_granted_source_links_preserve_exact_spelling_and_rebase_on_copy() {
    configured(|| {
        let (temp, workspace, context) = fixture();
        let external = temp.path().join("PrivateGrantedSite316");
        std::fs::create_dir_all(external.join("pages")).unwrap();
        std::fs::create_dir(external.join("assets")).unwrap();
        std::fs::write(external.join("assets/logo.svg"), "<svg/>").unwrap();
        let page = external.join("pages/index.html");
        let original = "<main><img src='../assets/logo%2Esvg?q=2#mark'><img src=\"/assets/logo.svg?root=1#icon\"></main>";
        std::fs::write(&page, original).unwrap();
        let mut authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
        authority.add_root(&external, false).unwrap();
        context.authorize_tool_roots(&authority).unwrap();
        let projected = context
            .project_source(page.to_str().unwrap(), original)
            .unwrap();
        assert!(!projected.text.contains("logo.svg"));
        let reference = context.project_filename_path(&page).unwrap();
        let restored = context
            .prepare_edit_ticket(
                "write_file",
                &json!({"path":reference,"content":projected.text}),
            )
            .unwrap();
        assert_eq!(restored["content"], original);
        let mut names = context.state.lock().unwrap();
        let names = names.filenames.as_mut().unwrap();
        let copied = names
            .restore_source_text(
                &projected.text,
                Some(workspace.join("copied.html").to_str().unwrap()),
            )
            .unwrap();
        assert!(copied.contains("logo.svg?q=2#mark"), "{copied}");
        assert!(copied.contains("logo.svg?root=1#icon"), "{copied}");
        assert!(
            copied.contains("../PrivateGrantedSite316/assets/"),
            "{copied}"
        );
    });
}

#[test]
fn builtin_preserved_code_filename_spelling_is_not_mistaken_for_a_path() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        std::fs::create_dir(workspace.join("PrivateClass316")).unwrap();
        context.refresh_filenames().unwrap();
        let path = workspace.join("PrivatePage316.html");
        let original = "<div class='PrivateClass316'><p>private text</p></div>";
        std::fs::write(&path, original).unwrap();
        let projected = context
            .project_source("PrivatePage316.html", original)
            .unwrap();
        assert!(!projected.text.contains("PrivateClass316"));
        let reference = context.project_filename_path(&path).unwrap();
        let restored = context
            .prepare_edit_ticket(
                "write_file",
                &json!({"path":reference,"content":projected.text}),
            )
            .unwrap();
        assert_eq!(restored["content"], original);
    });
}

#[test]
fn builtin_generated_public_name_keeps_prior_provider_history_stable() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        let authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
        context.authorize_tool_roots(&authority).unwrap();
        let message = crate::agent::client::ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("Build public-output316.js"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        };
        let original = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::Headless,
            "test",
            "model",
            vec![message],
            Vec::new(),
            Default::default(),
        );
        let mut before = original.clone();
        context.prepare_request(&mut before).unwrap();
        let args = context
            .prepare_edit_ticket(
                "run_command",
                &json!({"command":"printf build > public-output316.js"}),
            )
            .unwrap();
        let command = context
            .prepare_command_workspace(
                args["command"].as_str().unwrap(),
                workspace.to_str().unwrap(),
                std::slice::from_ref(&workspace),
            )
            .unwrap();
        std::fs::write(Path::new(&command.cwd).join("public-output316.js"), "build").unwrap();
        let mut after = original;
        context.prepare_request(&mut after).unwrap();
        assert_eq!(
            serde_json::to_value(before.messages).unwrap(),
            serde_json::to_value(after.messages).unwrap()
        );
        // Existing private collisions cannot be declared public by a command.
        let private = context
            .project_filename_path(&workspace.join("PrivatePage316.html"))
            .unwrap();
        context
            .state
            .lock()
            .unwrap()
            .filenames
            .as_mut()
            .unwrap()
            .declare_public_names("PrivatePage316.html")
            .unwrap();
        assert_eq!(context.mask_known("PrivatePage316.html").unwrap(), private);
    });
}

#[test]
fn builtin_nested_revoked_paths_cannot_restore_saved_authority() {
    configured(|| {
        let (temp, workspace, context) = fixture();
        let external = temp.path().join("external");
        std::fs::create_dir(&external).unwrap();
        std::fs::write(external.join("PrivateLog316.txt"), "log").unwrap();
        let mut authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
        authority.add_root(&external, false).unwrap();
        context.authorize_tool_roots(&authority).unwrap();
        let reference = context
            .project_filename_path(&external.join("PrivateLog316.txt"))
            .unwrap();
        context
            .authorize_tool_roots(
                &crate::workspace::WorkspaceContext::discover(&workspace).unwrap(),
            )
            .unwrap();
        for name in [
            "watch_process",
            "configure_agent_graph",
            "spawn_agent_batch",
        ] {
            assert!(
                context
                    .prepare_edit_ticket(name, &json!({"nested":{"path":reference}}))
                    .is_err(),
                "{name}"
            );
        }
    });
}

#[test]
fn builtin_command_generation_binds_bytes_even_when_size_and_mtime_match() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        context
            .authorize_tool_roots(
                &crate::workspace::WorkspaceContext::discover(&workspace).unwrap(),
            )
            .unwrap();
        let first = context
            .prepare_command_workspace(
                "true",
                workspace.to_str().unwrap(),
                std::slice::from_ref(&workspace),
            )
            .unwrap();
        let path = workspace.join("PrivatePage316.html");
        let metadata = std::fs::metadata(&path).unwrap();
        std::fs::write(&path, "x".repeat(metadata.len() as usize)).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
            .unwrap();
        let second = context
            .prepare_command_workspace(
                "true",
                workspace.to_str().unwrap(),
                std::slice::from_ref(&workspace),
            )
            .unwrap();
        assert_ne!(first.generation, second.generation);
    });
}

#[cfg(unix)]
#[test]
fn builtin_command_absolute_in_scope_link_targets_its_writable_clone() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        std::os::unix::fs::symlink(
            workspace.join("PrivatePage316.html"),
            workspace.join("link.html"),
        )
        .unwrap();
        context
            .authorize_tool_roots(
                &crate::workspace::WorkspaceContext::discover(&workspace).unwrap(),
            )
            .unwrap();
        let command = context
            .prepare_command_workspace(
                "true",
                workspace.to_str().unwrap(),
                std::slice::from_ref(&workspace),
            )
            .unwrap();
        std::fs::write(
            Path::new(&command.cwd).join("PrivatePage316.html"),
            "clone contents",
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(Path::new(&command.cwd).join("link.html")).unwrap(),
            "clone contents"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("PrivatePage316.html")).unwrap(),
            "<p>private text</p>\n"
        );
    });
}

#[cfg(target_os = "macos")]
#[test]
fn builtin_guarded_native_commands_keep_sources_and_support_background_artifacts() {
    configured(|| {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let (_temp, workspace, context) = fixture();
            let authority = crate::workspace::WorkspaceContext::discover(&workspace).unwrap();
            context.authorize_tool_roots(&authority).unwrap();
            let owner = crate::jobs::JobOwner::new(Some(context.session_id));
            let native = crate::agent::registry::ToolContext::native_read(&authority, 10, crate::agent::registry::ToolProfile::Main, crate::sandbox::SandboxMode::Required, owner);
            super::scope(Some(context.clone()), async {
                let alias = context.project_filename_path(&workspace.join("PrivatePage316.html")).unwrap();
                let root_reference = context.project_filename_path(&workspace).unwrap();
                let model_command = format!("printf 'isolated' > '{alias}'; printf 'artifact' > '{root_reference}/build-result316.txt'");
                let args = context.prepare_edit_ticket("run_command", &json!({"command":model_command,"cwd":"."})).unwrap();
                assert_eq!(context.mask_known(&model_command).unwrap(), model_command, "reserved output references remain valid in provider history");
                let result = crate::agent::tools::execute_tool_with_context("run_command", &args, &native).await;
                assert!(result.is_success(), "{}", result.model_text());
                assert_eq!(std::fs::read_to_string(workspace.join("PrivatePage316.html")).unwrap(), "<p>private text</p>\n");
                let roots = context.command_artifact_roots(); assert_eq!(roots.len(), 1);
                let artifact = roots[0].join("build-result316.txt"); assert_eq!(std::fs::read_to_string(&artifact).unwrap(), "artifact");
                context.refresh_filenames().unwrap();
                let artifact_directory = result.model_text().lines().find_map(|line| line.strip_prefix("command_artifact_directory: ")).expect("native result publishes owned artifact directory").to_owned();
                assert!(!artifact_directory.is_empty());
                let listing = crate::agent::tools::execute_tool_with_context("list_dir", &json!({"path":artifact_directory}), &native).await;
                assert!(listing.is_success(), "{}", listing.model_text());
                let provider_listing = projected_listing(&context, &artifact_directory, listing.model_text()).unwrap();
                let reference = provider_listing.lines().find(|line| line.ends_with(".txt")).expect("generated file is discoverable through provider listing").to_owned();
                let read = crate::agent::tools::execute_tool_with_context("read_file", &json!({"path":reference}), &native).await;
                assert!(read.is_success(), "{}", read.model_text()); assert!(read.model_text().contains("artifact"));
                let command = context.prepare_edit_ticket("run_command", &json!({"command":format!("cat '{reference}' > sequential-result316.txt"),"cwd":artifact_directory})).unwrap();
                let sequential = crate::agent::tools::execute_tool_with_context("run_command", &command, &native).await;
                assert!(sequential.is_success(), "{}", sequential.model_text());
                assert_eq!(std::fs::read_to_string(roots[0].join("sequential-result316.txt")).unwrap(), "artifact");
                assert_eq!(context.command_artifact_roots(), roots);
                assert!(!workspace.join("sequential-result316.txt").exists());
                let command = context.prepare_edit_ticket("run_command", &json!({"command":"sleep 0.05; printf 'background' > background-result316.txt","background":true})).unwrap();
                let started = crate::agent::tools::execute_tool_with_context("run_command", &command, &native).await;
                assert!(started.is_success(), "{}", started.model_text());
                let id = started.model_text().split_whitespace().last().unwrap();
                let finished = crate::agent::tools::execute_tool_with_context("await_job", &json!({"job_id":id,"timeout_secs":5}), &native).await;
                assert!(finished.is_success(), "{}", finished.model_text());
                assert_eq!(std::fs::read_to_string(roots[0].join("background-result316.txt")).unwrap(), "background");
                assert!(!workspace.join("build-result316.txt").exists()); assert!(!workspace.join("background-result316.txt").exists());
            }).await;
        });
    });
}

#[test]
fn builtin_native_ids_and_public_controls_survive_filename_collisions() {
    configured(|| {
        let (_temp, workspace, context) = fixture();
        context.allow_ephemeral();
        for name in [
            "JOB",
            "WATCH",
            "AGENT",
            "completed",
            "ask",
            "type",
            "OpaqueReasoning316",
        ] {
            std::fs::create_dir(workspace.join(name)).unwrap();
        }
        context.refresh_filenames().unwrap();
        for (kind, tool, field) in [
            ("JOB", "await_job", "job_id"),
            ("WATCH", "stop_watcher", "watch_id"),
        ] {
            let id = Uuid::new_v4();
            context.bind_native_id(kind, id).unwrap();
            let token = context.mask_known(&id.to_string()).unwrap();
            assert!(token.contains("__CA_PRIVATE_"));
            assert_eq!(context.mask_known(&token).unwrap(), token);
            let prepared = context
                .prepare_edit_ticket(tool, &json!({field:token}))
                .unwrap();
            assert_eq!(prepared[field], id.to_string());
        }
        let id = Uuid::new_v4();
        let source = context
            .record_native_control(
                "native body",
                native_control::TypedNativeControl {
                    agents: vec![native_control::NativeAgentControl {
                        id,
                        status: native_control::NativeAgentStatus::Completed,
                        narrow_repair: None,
                    }],
                    batch: None,
                },
            )
            .unwrap();
        let projected = context
            .project_retrieved("native body", Some(&source))
            .unwrap();
        assert_eq!(context.mask_known(&projected).unwrap(), projected);
        let token = context.mask_known(&id.to_string()).unwrap();
        assert_eq!(
            context
                .prepare_edit_ticket("cancel_agent", &json!({"agent_id":token}))
                .unwrap()["agent_id"],
            id.to_string()
        );
        let mut controls = json!({"mode":"ask"});
        arguments::project(&context, "switch_mode", &mut controls).unwrap();
        assert_eq!(controls, json!({"mode":"ask"}));
        arguments::validate(&context, Some("switch_mode"), &controls).unwrap();
        let tools = crate::agent::registry::ToolRegistry::builtins()
            .specs()
            .iter()
            .map(|spec| json!({"type":"function","name":spec.name.as_str(),"parameters":spec.schema}))
            .collect::<Vec<_>>();
        let mut body = json!({"model":"type","tools":tools,"input":[{"type":"reasoning","encrypted_content":"OpaqueReasoning316","id":"completed"},{"type":"message","role":"user","content":"public request"}]});
        context
            .validate_wire(
                &serde_json::to_vec(&body).unwrap(),
                context.policy().revision,
                &context.policy_fingerprint(),
            )
            .unwrap();
        body["input"][1]["content"] = json!("PrivatePage316.html");
        assert!(context
            .validate_wire(
                &serde_json::to_vec(&body).unwrap(),
                context.policy().revision,
                &context.policy_fingerprint()
            )
            .is_err());
    });
}
