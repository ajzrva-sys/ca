use super::*;
use serde_json::json;

fn site() -> (tempfile::TempDir, PathBuf, Arc<Context>) {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let scope = root.path().join("profile");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&scope).unwrap();
    let policy_path = scope.join("privacy/projects/site/policy.json");
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

fn configured(run: impl FnOnce()) {
    let config = crate::provider_filter::ProviderFilterConfig {
        enabled: true,
        hide: crate::provider_filter::FilterHide {
            filenames: true,
            ..Default::default()
        },
        ..Default::default()
    };
    crate::provider_filter::with_test_filter(&config, run);
}

#[test]
fn whole_site_standalone_css_and_javascript_mask_data_preserve_logic() {
    configured(|| {
        let (_root, workspace, context) = site();
        for (path, source, private, public) in [
            (
                "style.css",
                "/* PRIVATE_COMMENT_SITE */\n@import './PRIVATE_THEME_SITE.css';\n.card { color: red; padding: 16px; background: url('../PRIVATE_ASSET_SITE.png'); }\n.card::after { content: 'PRIVATE_LABEL_SITE'; }\n",
                vec!["PRIVATE_COMMENT_SITE", "PRIVATE_THEME_SITE", "PRIVATE_ASSET_SITE", "PRIVATE_LABEL_SITE"],
                vec![".card", "color: red", "padding: 16px"],
            ),
            (
                "app.js",
                "import data from './PRIVATE_DATA_SITE.json';\nconst customer = 'PRIVATE_CUSTOMER_SITE';\ndocument.querySelector('.card').style.color = customer;\n",
                vec!["PRIVATE_DATA_SITE", "PRIVATE_CUSTOMER_SITE"],
                vec!["import data from", "const customer =", "querySelector('.card')", "style.color = customer"],
            ),
        ] {
            std::fs::write(workspace.join(path), source).unwrap();
            context.refresh_filenames().unwrap();
            let projected = context.project_source(path, source).unwrap();
            let sent = context.mask_known(&projected.text).unwrap();
            for value in private {
                assert!(!sent.contains(value), "{path} leaked {value}: {sent}");
            }
            for value in public {
                assert!(sent.contains(value), "{path} lost {value}: {sent}");
            }
            let file = context.mask_known(path).unwrap();
            let args = context.prepare_edit_ticket("write_file", &json!({"path":file,"content":sent})).unwrap();
            assert_eq!(args["content"], source);
            context.consume_edit_ticket("write_file", &args).unwrap();
        }
    });
}

#[test]
fn whole_site_link_alias_opens_target_and_restores_each_original_spelling() {
    configured(|| {
        let (_root, workspace, context) = site();
        std::fs::create_dir(workspace.join("PrivatePagesSite")).unwrap();
        std::fs::create_dir(workspace.join("PrivateAssetsSite")).unwrap();
        let asset = workspace.join("PrivateAssetsSite/logo.svg");
        std::fs::write(&asset, "<svg/>").unwrap();
        let name = "PrivatePagesSite/index.html";
        let source = "<main class='card'><img src='../PrivateAssetsSite/logo.svg?size=2#mark'><img src='/PrivateAssetsSite/logo.svg'></main>";
        std::fs::write(workspace.join(name), source).unwrap();
        context.refresh_filenames().unwrap();
        let projection = context.project_source(name, source).unwrap();
        let sent = context.mask_known(&projection.text).unwrap();
        assert!(!sent.contains("PrivateAssetsSite"));
        for spelling in [
            "../PrivateAssetsSite/logo.svg?size=2#mark",
            "/PrivateAssetsSite/logo.svg",
        ] {
            let link = projection
                .changes
                .iter()
                .find(|change| change.original == spelling)
                .unwrap();
            let args = context
                .prepare_edit_ticket("read_file", &json!({"path":link.replacement}))
                .unwrap();
            assert_eq!(args["path"], asset.to_string_lossy().as_ref());
        }
        let path = context.mask_known(name).unwrap();
        let args = context
            .prepare_edit_ticket(
                "write_file",
                &json!({"path":path,"content":sent.replace("class='card'", "class='card wide'")}),
            )
            .unwrap();
        assert_eq!(
            args["content"],
            source.replace("class='card'", "class='card wide'")
        );
        context.consume_edit_ticket("write_file", &args).unwrap();
        assert_eq!(std::fs::read_to_string(asset).unwrap(), "<svg/>");
    });
}

#[test]
fn whole_site_new_file_keeps_directory_private_and_rejects_traversal() {
    configured(|| {
        let (_root, workspace, context) = site();
        std::fs::create_dir(workspace.join("PrivateNewDirectorySite")).unwrap();
        context.refresh_filenames().unwrap();
        let directory = context.mask_known("PrivateNewDirectorySite").unwrap();
        let path = format!("{directory}/new.css");
        let args = context
            .prepare_edit_ticket(
                "write_file",
                &json!({"path":path,"content":".card { color: blue; }"}),
            )
            .unwrap();
        assert_eq!(
            args["path"],
            workspace
                .join("PrivateNewDirectorySite/new.css")
                .to_string_lossy()
                .as_ref()
        );
        context.consume_edit_ticket("write_file", &args).unwrap();
        std::fs::write(
            args["path"].as_str().unwrap(),
            args["content"].as_str().unwrap(),
        )
        .unwrap();
        context.refresh_filenames().unwrap();
        assert_eq!(
            context
                .prepare_edit_ticket("read_file", &json!({"path":path}))
                .unwrap()["path"],
            args["path"]
        );
        for invalid in [
            format!("{directory}/../escaped.css"),
            format!("{directory}/__CA_NAME_forged__"),
            format!("{directory}/new.css/child.css"),
        ] {
            assert!(context
                .prepare_edit_ticket("write_file", &json!({"path":invalid,"content":"x"}))
                .is_err());
        }
    });
}

#[test]
fn whole_site_svg_asset_data_hidden_and_type_preserved() {
    configured(|| {
        let (_root, workspace, context) = site();
        let source = "<svg viewBox='0 0 20 20'><title>PRIVATE_SVG_TITLE_SITE</title><path fill='red' d='M0 0h20v20H0z'/></svg>";
        std::fs::write(workspace.join("PrivateLogoSite.svg"), source).unwrap();
        context.refresh_filenames().unwrap();
        let path = context.mask_known("PrivateLogoSite.svg").unwrap();
        assert!(path.ends_with(".svg"));
        let projection = context
            .project_source("PrivateLogoSite.svg", source)
            .unwrap();
        assert!(!projection.text.contains("PRIVATE_SVG_TITLE_SITE"));
        let args = context
            .prepare_edit_ticket(
                "write_file",
                &json!({"path":path,"content":projection.text}),
            )
            .unwrap();
        assert_eq!(args["content"], source);
    });
}

#[test]
fn whole_site_copied_link_and_new_asset_link_keep_the_same_target() {
    configured(|| {
        let (_root, workspace, context) = site();
        for directory in ["first", "second"] {
            std::fs::create_dir(workspace.join(directory)).unwrap();
            std::fs::write(
                workspace.join(directory).join("detail.html"),
                "<p>local detail</p>",
            )
            .unwrap();
            std::fs::write(
                workspace.join(directory).join("index.html"),
                "<main></main>",
            )
            .unwrap();
        }
        let first = "<a href='./detail.html?size=2#mark'>PRIVATE_LINK_SITE</a>";
        std::fs::write(workspace.join("first/index.html"), first).unwrap();
        context.refresh_filenames().unwrap();
        let projected = context.project_source("first/index.html", first).unwrap();
        context
            .project_source("second/index.html", "<main></main>")
            .unwrap();
        let second = context.mask_known("second/index.html").unwrap();
        let args = context
            .prepare_edit_ticket(
                "write_file",
                &json!({"path":second,"content":projected.text}),
            )
            .unwrap();
        assert_eq!(
            args["content"],
            first.replace("./detail.html", "../first/detail.html")
        );
        let target = context.mask_known("first/detail.html").unwrap();
        let args = context.prepare_edit_ticket("edit_file", &json!({"path":second,"old_string":"<main></main>","new_string":format!("<a href='{target}'>New link</a>")})).unwrap();
        assert_eq!(
            args["new_string"],
            "<a href='../first/detail.html'>New link</a>"
        );
    });
}

#[test]
fn whole_site_jsx_tsx_text_and_form_values_are_private() {
    configured(|| {
        let (_root, workspace, context) = site();
        for name in ["app.jsx", "app.tsx"] {
            let source = "const page = <section className=\"card\" id=\"dashboard\"><h1>PRIVATE_JSX_TITLE_SITE</h1><input value=\"PRIVATE_JSX_FORM_SITE\" /></section>;\n";
            std::fs::write(workspace.join(name), source).unwrap();
            context.refresh_filenames().unwrap();
            let projected = context.project_source(name, source).unwrap();
            assert!(
                !projected.text.contains("PRIVATE_JSX_TITLE_SITE"),
                "{name}: {}",
                projected.text
            );
            assert!(!projected.text.contains("PRIVATE_JSX_FORM_SITE"));
            assert!(projected.text.contains("className=\"card\""));
            assert!(projected.text.contains("id=\"dashboard\""));
            let path = context.mask_known(name).unwrap();
            let args = context
                .prepare_edit_ticket("write_file", &json!({"path":path,"content":projected.text}))
                .unwrap();
            assert_eq!(args["content"], source);
        }
    });
}

fn site_request(
    messages: Vec<crate::agent::client::ChatMessage>,
) -> crate::request::PreparedModelRequest {
    crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ToolContinuation,
        "site-test",
        "model",
        messages,
        vec![],
        Default::default(),
    )
}

fn site_message(role: &str, text: String) -> crate::agent::client::ChatMessage {
    crate::agent::client::ChatMessage {
        role: role.into(),
        content: crate::image_content::ModelContent::text(text),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }
}

#[test]
fn whole_site_authored_relocation_paths_keep_executable_filename_references() {
    configured(|| {
        let (_root, workspace, context) = site();
        let source = "PrivatePagesRelocation/index.html";
        let target = "PrivatePagesRelocation/PrivateNestedRelocation";
        std::fs::create_dir_all(workspace.join(target)).unwrap();
        std::fs::write(workspace.join(source), "<main></main>").unwrap();
        context.refresh_filenames().unwrap();
        let source_alias = context.mask_known(source).unwrap();
        let target_alias = context.mask_known(target).unwrap();
        for suffix in ["", "; export function decorate(value) { return value; }"] {
            let mut request = site_request(vec![site_message(
                "user",
                format!("Move @{source} into \"{target}\" using native file tools{suffix}"),
            )]);
            context.prepare_request(&mut request).unwrap();
            let authored = request.messages[0].content.as_str().unwrap();
            assert!(authored.contains(&source_alias), "{authored}");
            assert!(authored.contains(&target_alias), "{authored}");
            assert!(!authored.contains("PrivatePagesRelocation"));
            assert_eq!(request.messages.len(), 1);
            assert_eq!(
                context
                    .prepare_edit_ticket("read_file", &json!({"path":source_alias}))
                    .unwrap()["path"],
                workspace.join(source).to_string_lossy().as_ref()
            );
            assert_eq!(
                context
                    .prepare_edit_ticket("list_dir", &json!({"path":target_alias}))
                    .unwrap()["path"],
                workspace.join(target).to_string_lossy().as_ref()
            );
        }
    });
}

#[test]
fn whole_site_failed_directory_result_remains_replayable_diagnostic() {
    configured(|| {
        let (_root, _workspace, context) = site();
        for diagnostic in [
            "[tool failed]\nUse a complete discovered filename alias; local action was blocked",
            "Error: directory is unavailable",
        ] {
            let mut assistant = site_message("assistant", String::new());
            assistant.tool_calls = Some(vec![crate::agent::client::ToolCallMsg {
                id: "failed-site-list".into(),
                call_type: "function".into(),
                function: crate::agent::client::FunctionCall {
                    name: "list_dir".into(),
                    arguments: json!({"path":"folder_00000000000000000000000000000001"})
                        .to_string(),
                    arguments_retrieved_spans: Vec::new(),
                },
            }]);
            let mut result = site_message("tool", diagnostic.into());
            result.tool_call_id = Some("failed-site-list".into());
            let mut request = site_request(vec![assistant, result]);
            context.prepare_request(&mut request).unwrap();
            assert_eq!(request.messages[1].content.as_str().unwrap(), diagnostic);
        }
    });
}

#[test]
fn whole_site_creation_keeps_already_disclosed_leaf_history_stable() {
    configured(|| {
        let (_root, workspace, context) = site();
        std::fs::create_dir(workspace.join("PrivateCreationTarget")).unwrap();
        let authored = "Create extra.js in \"PrivateCreationTarget\"; export function decorate(value) { return value; }";
        let mut before = site_request(vec![site_message("user", authored.into())]);
        context.prepare_request(&mut before).unwrap();
        let sent_before = serde_json::to_value(&before.messages).unwrap();
        assert!(sent_before.to_string().contains("extra.js"));
        assert!(!sent_before.to_string().contains("PrivateCreationTarget"));
        let directory = context.mask_known("PrivateCreationTarget").unwrap();
        let args = context.prepare_edit_ticket("write_file", &json!({"path":format!("{directory}/extra.js"),"content":"export function decorate(value) { return value; }\n"})).unwrap();
        context.consume_edit_ticket("write_file", &args).unwrap();
        std::fs::write(
            args["path"].as_str().unwrap(),
            args["content"].as_str().unwrap(),
        )
        .unwrap();
        let mut after = site_request(vec![site_message("user", authored.into())]);
        context.prepare_request(&mut after).unwrap();
        assert_eq!(serde_json::to_value(&after.messages).unwrap(), sent_before);
        let state = context.state.lock().unwrap();
        let names = state.filenames.as_ref().unwrap();
        let mut loaded =
            filenames::FilenameAliases::from_snapshot(workspace.clone(), names.snapshot()).unwrap();
        loaded.discover().unwrap();
        assert_eq!(loaded.project_text("extra.js").unwrap(), "extra.js");
        let full = loaded
            .project_text("PrivateCreationTarget/extra.js")
            .unwrap();
        assert!(!full.contains("PrivateCreationTarget"));
        assert_eq!(
            loaded.resolve_path(&full).unwrap(),
            workspace.join("PrivateCreationTarget/extra.js")
        );
    });
}

#[test]
fn whole_site_creation_does_not_disclose_preexisting_private_basename() {
    configured(|| {
        let (_root, workspace, context) = site();
        for directory in ["PrivateExistingTarget", "PrivateNewTarget"] {
            std::fs::create_dir(workspace.join(directory)).unwrap();
        }
        std::fs::write(
            workspace.join("PrivateExistingTarget/SecretExistingModule.js"),
            "export const value = 1;\n",
        )
        .unwrap();
        context.refresh_filenames().unwrap();
        let directory = context.mask_known("PrivateNewTarget").unwrap();
        let reference = format!("{directory}/SecretExistingModule.js");
        let before = context
            .state
            .lock()
            .unwrap()
            .filenames
            .as_ref()
            .unwrap()
            .snapshot();
        assert!(context
            .prepare_edit_ticket(
                "write_file",
                &json!({"path":reference,"content":"export const value = 2;\n"})
            )
            .is_err());
        assert_eq!(
            context
                .state
                .lock()
                .unwrap()
                .filenames
                .as_ref()
                .unwrap()
                .snapshot(),
            before
        );
        assert!(!context
            .mask_known("SecretExistingModule.js")
            .unwrap()
            .contains("SecretExistingModule"));
        assert!(context.mask_known(&reference).is_err());
    });
}

#[test]
fn whole_site_historical_link_stays_stable_after_rename_removes_target() {
    configured(|| {
        let (_root, workspace, context) = site();
        let source = "<link rel='stylesheet' href='./PrivateOriginalStyles.css?theme=2#main'>";
        std::fs::write(workspace.join("index.html"), source).unwrap();
        std::fs::write(
            workspace.join("PrivateOriginalStyles.css"),
            ".card { color: red; }\n",
        )
        .unwrap();
        context.refresh_filenames().unwrap();
        let before = context.project_source("index.html", source).unwrap();
        let link = before
            .changes
            .iter()
            .find(|change| change.original.contains("PrivateOriginalStyles.css"))
            .unwrap()
            .replacement
            .clone();
        std::fs::rename(
            workspace.join("PrivateOriginalStyles.css"),
            workspace.join("renamed.css"),
        )
        .unwrap();
        context.refresh_filenames().unwrap();
        let after = context.project_source("index.html", source).unwrap();
        assert_eq!(after.text, before.text);
        assert!(context
            .prepare_edit_ticket("read_file", &json!({"path":link}))
            .is_err());
    });
}

#[test]
fn whole_site_new_directory_batch_moves_keep_private_filenames_and_history() {
    configured(|| {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let (_root, workspace, context) = site();
            for folder in ["PrivateSourceDirs315", "PrivateDeploymentDirs315"] {
                std::fs::create_dir(workspace.join(folder)).unwrap();
            }
            let name = "PrivateModuleOriginal315.js";
            std::fs::write(workspace.join("PrivateSourceDirs315").join(name), "export const value = 1;\n").unwrap();
            context.refresh_filenames().unwrap();
            let source = context.mask_known("PrivateSourceDirs315").unwrap();
            let parent = context.mask_known("PrivateDeploymentDirs315").unwrap();
            let authored = "Move the JS files from \"PrivateSourceDirs315\" into new site/scripts beneath \"PrivateDeploymentDirs315\"";
            let mut before = site_request(vec![site_message("user", authored.into())]);
            context.prepare_request(&mut before).unwrap();
            let before = serde_json::to_value(&before.messages).unwrap();
            let workspace_context = crate::workspace::WorkspaceContext::from_primary(workspace.clone(), workspace.clone());
            super::scope(Some(context.clone()), async {
                for tool in ["create_directory", "select_files", "move_files"] {
                    assert!(context.check_filename_tool(tool).is_ok(), "{tool} unavailable");
                }
                async fn execute(context: &Arc<Context>, workspace: &crate::workspace::WorkspaceContext, name: &str, args: serde_json::Value) -> String {
                    let rules = [crate::config::ApprovalRule::AllowTool(name.into())];
                    let broker = crate::approval::ApprovalBroker::new(workspace, &rules, crate::approval::ApprovalOrigin::Main);
                    let (prepared, decision) = broker.prepare_formatted(name, &args, crate::agent::registry::ToolOrigin::BuiltIn, false, crate::sandbox::SandboxMode::Required).await.unwrap();
                    assert_eq!(decision, crate::approval::BrokerDecision::AllowedByRule);
                    let tool_context = crate::agent::registry::ToolContext::native_read(workspace, 30, crate::agent::registry::ToolProfile::Main, crate::sandbox::SandboxMode::Required, crate::jobs::JobOwner::new(Some(context.session_id)));
                    let result = crate::agent::tools::execute_tool_with_context(name, &prepared.arguments, &tool_context).await;
                    assert!(result.is_success(), "{}", result.model_text());
                    result.model_text().to_string()
                }
                execute(&context, &workspace_context, "create_directory", json!({"path":format!("{parent}/site")})).await;
                context.refresh_filenames().unwrap();
                let site_alias = context.mask_known("PrivateDeploymentDirs315/site").unwrap();
                execute(&context, &workspace_context, "create_directory", json!({"path":format!("{site_alias}/scripts")})).await;
                context.refresh_filenames().unwrap();
                let destination = context.mask_known("PrivateDeploymentDirs315/site/scripts").unwrap();
                let selected: serde_json::Value = serde_json::from_str(&execute(&context, &workspace_context, "select_files", json!({"folder":source,"destination":destination,"criteria":{"extensions":["js"]}})).await).unwrap();
                assert_eq!(selected["count"], 1);
                let move_args = json!({"selection":selected["selection"],"destination":selected["destination"]});
                let denied = crate::workspace::WorkspaceContext::from_primary(workspace.join("DifferentRoot"), workspace.clone());
                let rules = [crate::config::ApprovalRule::AllowTool("move_files".into())];
                let broker = crate::approval::ApprovalBroker::new(&denied, &rules, crate::approval::ApprovalOrigin::Main);
                assert!(broker.prepare_formatted("move_files", &move_args, crate::agent::registry::ToolOrigin::BuiltIn, false, crate::sandbox::SandboxMode::Required).await.is_err());
                let moved: serde_json::Value = serde_json::from_str(&execute(&context, &workspace_context, "move_files", move_args).await).unwrap();
                assert_eq!(moved["moved"], 1);
                assert_eq!(std::fs::read(workspace.join("PrivateDeploymentDirs315/site/scripts").join(name)).unwrap(), b"export const value = 1;\n");
                assert!(!workspace.join("PrivateSourceDirs315").join(name).exists());
                let mut after = site_request(vec![site_message("user", authored.into())]);
                context.prepare_request(&mut after).unwrap();
                assert_eq!(serde_json::to_value(&after.messages).unwrap(), before);
                assert!(context.prepare_edit_ticket("select_files", &json!({"folder":"PrivateSourceDirs315"})).is_err());
                assert!(context.prepare_edit_ticket("select_files", &json!({"folder":"folder_00000000000000000000000000000000"})).is_err());
            }).await;
        });
    });
}

#[test]
fn whole_site_new_root_directory_keeps_disclosed_leaf_history_stable() {
    configured(|| {
        let (_root, workspace, context) = site();
        context.refresh_filenames().unwrap();
        let mut before = site_request(vec![site_message(
            "user",
            "Create a ./newsite directory".into(),
        )]);
        let mut create_call = site_message("assistant", String::new());
        create_call.tool_calls = Some(vec![crate::agent::client::ToolCallMsg {
            id: "create-root-directory".into(),
            call_type: "function".into(),
            function: crate::agent::client::FunctionCall {
                name: "create_directory".into(),
                arguments: json!({"path":"./newsite"}).to_string(),
                arguments_retrieved_spans: Vec::new(),
            },
        }]);
        before.messages.push(create_call.clone());
        context.prepare_request(&mut before).unwrap();
        let args = context
            .prepare_edit_ticket("create_directory", &json!({"path":"./newsite"}))
            .unwrap();
        std::fs::create_dir(args["path"].as_str().unwrap()).unwrap();
        let mut after = site_request(vec![site_message(
            "user",
            "Create a ./newsite directory".into(),
        )]);
        after.messages.push(create_call);
        context.prepare_request(&mut after).unwrap();
        assert_eq!(
            serde_json::to_value(&after.messages).unwrap(),
            serde_json::to_value(&before.messages).unwrap()
        );
        let state = context.state.lock().unwrap();
        let names = state.filenames.as_ref().unwrap();
        let alias = names.project_path("newsite").unwrap();
        assert_eq!(
            names.resolve_path(&alias).unwrap(),
            workspace.join("newsite")
        );
        assert!(!names
            .project_text(&workspace.join("newsite").to_string_lossy())
            .unwrap()
            .contains("newsite"));
    });
}

#[test]
fn whole_site_new_same_directory_module_link_is_explicitly_relative() {
    configured(|| {
        let (_root, workspace, context) = site();
        std::fs::create_dir(workspace.join("PrivateModules315")).unwrap();
        std::fs::write(
            workspace.join("PrivateModules315/PrivateShared315.js"),
            "export const value = 1;\n",
        )
        .unwrap();
        std::fs::write(
            workspace.join("PrivateModules315/entry.js"),
            "import { value } from \"./PrivateShared315.js\";\n",
        )
        .unwrap();
        context.refresh_filenames().unwrap();
        context
            .project_source(
                "PrivateModules315/entry.js",
                "import { value } from \"./PrivateShared315.js\";\n",
            )
            .unwrap();
        let target = context
            .mask_known("PrivateModules315/PrivateShared315.js")
            .unwrap();
        let replacement = context
            .prepare_edit_ticket(
                "edit_file",
                &json!({
                    "path":context.mask_known("PrivateModules315/entry.js").unwrap(),
                    "old_string":"./PrivateShared315.js",
                    "new_string":format!("./{target}")
                }),
            )
            .unwrap();
        assert_eq!(replacement["new_string"], "./PrivateShared315.js");
        for prefix in ["", "./"] {
            let args = context
                .prepare_edit_ticket("write_file", &json!({"path":context.mask_known("PrivateModules315/entry.js").unwrap(),"content":format!("import {{ value }} from \"{prefix}{target}\";\n")}))
                .unwrap();
            assert_eq!(
                args["content"],
                "import { value } from \"./PrivateShared315.js\";\n"
            );
        }
    });
}

#[test]
fn whole_site_native_write_preview_is_stable_when_corrected_html_is_read() {
    configured(|| {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let (_root, workspace, context) = site();
            for folder in ["PrivatePagesPreview315", "PrivateStylesPreview315", "PrivateScriptsPreview315", "PrivateAssetsPreview315", "PrivateDeployPreview315/site/pages", "PrivateDeployPreview315/site/styles", "PrivateDeployPreview315/site/scripts"] {
                std::fs::create_dir_all(workspace.join(folder)).unwrap();
            }
            let original = "<html><head><link rel=\"stylesheet\" href=\"../PrivateStylesPreview315/PrivateThemePreview315.css?theme=2#main\"></head><body><h1>PrivateTitlePreview315</h1><img src=\"../PrivateAssetsPreview315/PrivateBadgePreview315.png\"><script type=\"module\" src=\"../PrivateScriptsPreview315/PrivateAppPreview315.js\"></script></body></html>\n";
            let old_page = "PrivatePagesPreview315/PrivatePagePreview315.html";
            let new_page = "PrivateDeployPreview315/site/pages/PrivatePagePreview315.html";
            std::fs::write(workspace.join(old_page), original).unwrap();
            std::fs::write(workspace.join("PrivateStylesPreview315/PrivateThemePreview315.css"), ".card { color: red; }\n").unwrap();
            std::fs::write(workspace.join("PrivateScriptsPreview315/PrivateAppPreview315.js"), "export const value = 1;\n").unwrap();
            std::fs::write(workspace.join("PrivateAssetsPreview315/PrivateBadgePreview315.png"), b"binary fixture").unwrap();
            context.refresh_filenames().unwrap();
            context.project_source(old_page, original).unwrap();
            for (from, to) in [(old_page, new_page), ("PrivateStylesPreview315/PrivateThemePreview315.css", "PrivateDeployPreview315/site/styles/PrivateThemePreview315.css"), ("PrivateScriptsPreview315/PrivateAppPreview315.js", "PrivateDeployPreview315/site/scripts/PrivateAppPreview315.js")] {
                std::fs::rename(workspace.join(from), workspace.join(to)).unwrap();
            }
            context.refresh_filenames().unwrap();
            context.project_source(new_page, original).unwrap();
            let corrected = original.replace("../PrivateStylesPreview315/", "../styles/").replace("../PrivateScriptsPreview315/", "../scripts/").replace("../PrivateAssetsPreview315/", "../../../PrivateAssetsPreview315/");
            let path = context.mask_known(new_page).unwrap();
            let args = context.prepare_edit_ticket("write_file", &json!({"path":path,"content":corrected})).unwrap();
            let workspace_context = crate::workspace::WorkspaceContext::from_primary(workspace.clone(), workspace.clone());
            super::scope(Some(context.clone()), async {
                let tool_context = crate::agent::registry::ToolContext::native_read(&workspace_context, 30, crate::agent::registry::ToolProfile::Main, crate::sandbox::SandboxMode::Required, crate::jobs::JobOwner::new(Some(context.session_id)));
                let result = crate::agent::tools::execute_tool_with_context("write_file", &args, &tool_context).await;
                assert!(result.is_success(), "{}", result.model_text());
                assert_eq!(std::fs::read_to_string(workspace.join(new_page)).unwrap(), corrected);
                assert_eq!(context.state.lock().unwrap().views.get(new_page).unwrap().source_digest, digest(original.as_bytes()), "prospective classification must not grant a new edit view");
                let before_read = context.mask_known(result.model_text()).unwrap();
                context.record_read(workspace.join(new_page).to_str().unwrap(), &corrected, &corrected, 1, 500, false).unwrap();
                let after_read = context.mask_known(result.model_text()).unwrap();
                assert_eq!(after_read, before_read, "final HTML read rewrote the already sent native write preview");
            }).await;
        });
    });
}
