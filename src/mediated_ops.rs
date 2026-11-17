//! Host-mediated HTTP, Git, and SSH. The child names a destination. The host
//! pins the program and arguments and does not hand over credentials.
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// One request header. A named pair, matching the object item schema Grok
/// publishes for array fields. A `(String, String)` tuple becomes draft-07
/// `items: [schema, schema]`, which the Responses API rejects.
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub body: String,
    #[serde(default)]
    pub headers: Vec<HttpHeader>,
    pub destination: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GitOperation {
    pub repository: String,
    pub remote: String,
    pub operation: String,
    pub git_ref: String,
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SshOperation {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub command: String,
    pub destination: String,
}

pub fn plan_http(request: &HttpRequest) -> Result<PlannedCommand, String> {
    if request.method.is_empty() || !request.path.starts_with('/') || request.destination.is_empty()
    {
        return Err("http request is incomplete".into());
    }
    for header in &request.headers {
        if matches!(
            header.name.to_ascii_lowercase().as_str(),
            "authorization" | "proxy-authorization" | "cookie"
        ) {
            return Err("authentication headers stay on the host".into());
        }
    }
    Ok(PlannedCommand {
        program: PathBuf::from("http_request"),
        args: vec![
            request.method.clone(),
            request.path.clone(),
            request.destination.clone(),
        ],
    })
}

pub fn plan_git(request: &GitOperation) -> Result<PlannedCommand, String> {
    if request.repository.is_empty() || request.operation.is_empty() {
        return Err("git operation is incomplete".into());
    }
    if request.operation.contains("config") || request.path.contains("hooks") {
        return Err("repository config is not authority".into());
    }
    Ok(PlannedCommand {
        program: PathBuf::from("/usr/bin/git"),
        args: vec![
            "-c".into(),
            "core.hooksPath=/dev/null".into(),
            "-c".into(),
            "credential.helper=".into(),
            "-c".into(),
            "protocol.ext.allow=never".into(),
            request.operation.clone(),
            request.remote.clone(),
            request.git_ref.clone(),
        ],
    })
}

pub fn plan_ssh(request: &SshOperation) -> Result<PlannedCommand, String> {
    if request.user.is_empty() || request.host.is_empty() || request.command.is_empty() {
        return Err("ssh operation is incomplete".into());
    }
    if request.command.contains("ProxyCommand") || request.destination.is_empty() {
        return Err("ssh forwarding and alternate commands are not admitted".into());
    }
    Ok(PlannedCommand {
        program: PathBuf::from("/usr/bin/ssh"),
        args: vec![
            "-F".into(),
            "/dev/null".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "ForwardAgent=no".into(),
            "-o".into(),
            "ClearAllForwardings=yes".into(),
            "-p".into(),
            request.port.to_string(),
            format!("{}@{}", request.user, request.host),
            request.command.clone(),
        ],
    })
}

/// Run an effect once. A second call sees the confirmed result and does not
/// invoke the effect again. A failed publication after the effect leaves the
/// outcome unknown instead of repeating it.
#[allow(dead_code)]
pub fn perform_once(
    seen: &mut Option<Value>,
    effect: impl FnOnce() -> Result<Value, String>,
) -> Result<Value, String> {
    if let Some(existing) = seen.clone() {
        return Ok(existing);
    }
    let value = effect()?;
    *seen = Some(value.clone());
    Ok(value)
}

pub fn execute_named(name: &str, arguments: &Value) -> Option<Result<Value, String>> {
    match name {
        "http_request" => Some(execute_http(arguments)),
        "git_operation" => Some(execute_git(arguments)),
        "ssh_operation" => Some(execute_ssh(arguments)),
        _ => None,
    }
}

fn execute_http(arguments: &Value) -> Result<Value, String> {
    let request: HttpRequest =
        serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
    let planned = plan_http(&request)?;
    Ok(json!({
        "program": planned.program,
        "args": planned.args,
        "body": request.body,
    }))
}

fn execute_git(arguments: &Value) -> Result<Value, String> {
    let request: GitOperation =
        serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
    let planned = plan_git(&request)?;
    Ok(json!({ "program": planned.program, "args": planned.args }))
}

fn execute_ssh(arguments: &Value) -> Result<Value, String> {
    let request: SshOperation =
        serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
    let planned = plan_ssh(&request)?;
    Ok(json!({ "program": planned.program, "args": planned.args }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_operations_pin_argv_and_reject_host_credentials() {
        let http = plan_http(&HttpRequest {
            method: "GET".into(),
            path: "/once".into(),
            body: String::new(),
            headers: vec![],
            destination: "local-http".into(),
        })
        .unwrap();
        assert_eq!(http.args[0], "GET");
        assert!(plan_http(&HttpRequest {
            method: "GET".into(),
            path: "/once".into(),
            body: String::new(),
            headers: vec![HttpHeader {
                name: "Authorization".into(),
                value: "secret".into(),
            }],
            destination: "local-http".into(),
        })
        .is_err());
        let schema = serde_json::to_value(schemars::schema_for!(HttpRequest)).unwrap();
        let items = &schema["properties"]["headers"]["items"];
        let items = items
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .and_then(|reference| reference.strip_prefix("#/"))
            .and_then(|pointer| schema.pointer(&format!("/{pointer}")))
            .unwrap_or(items);
        assert_eq!(items["type"], "object");
        assert_eq!(items["properties"]["name"]["type"], "string");
        assert_eq!(items["properties"]["value"]["type"], "string");
        assert!(!items.get("items").is_some_and(serde_json::Value::is_array));
        let git = plan_git(&GitOperation {
            repository: "/work".into(),
            remote: "origin".into(),
            operation: "fetch".into(),
            git_ref: "main".into(),
            path: ".".into(),
        })
        .unwrap();
        assert!(git.args.iter().any(|arg| arg == "credential.helper="));
        assert!(git.args.iter().any(|arg| arg == "core.hooksPath=/dev/null"));
        let ssh = plan_ssh(&SshOperation {
            user: "runner".into(),
            host: "example.test".into(),
            port: 22,
            command: "true".into(),
            destination: "local-ssh".into(),
        })
        .unwrap();
        assert!(ssh.args.iter().any(|arg| arg == "ForwardAgent=no"));
        assert!(ssh.args.iter().any(|arg| arg == "/dev/null"));
    }

    #[test]
    fn one_effect_is_not_repeated() {
        let mut seen = None;
        let mut hits = 0;
        let first = perform_once(&mut seen, || {
            hits += 1;
            Ok(json!({"ok": true}))
        })
        .unwrap();
        let second = perform_once(&mut seen, || {
            hits += 1;
            Ok(json!({"ok": false}))
        })
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(hits, 1);
    }
}
