//! Anonymous provider contracts. Native paths and membership never belong here.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileCriteriaV1 {
    /// Match locally, including non-UTF-8 names. Case-insensitive, no leading dot.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Optional filename glob, evaluated by the host, never by a shell.
    #[serde(default)]
    pub name_glob: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectFilesV1 {
    pub folder: String,
    #[serde(default)]
    pub criteria: FileCriteriaV1,
    /// Exclude this destination subtree when it lies inside the source.
    #[serde(default)]
    pub destination: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveFilesV1 {
    pub selection: String,
    pub destination: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSelectionSummaryV1 {
    pub selection: String,
    pub version: u32,
    pub count: usize,
    pub types: std::collections::BTreeMap<String, usize>,
    pub folder: String,
    pub destination: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateResourceSummaryV1 {
    pub reference: String,
    pub kind: String,
    pub count: usize,
    pub types: std::collections::BTreeMap<String, usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateResourceCatalogV1 {
    pub schema: String,
    pub resources: Vec<PrivateResourceSummaryV1>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileMoveProgressV1 {
    pub operation: String,
    pub total: usize,
    pub moved: usize,
    pub skipped: usize,
    pub errors: usize,
    pub remaining: usize,
    pub cancelled: bool,
    /// Bounded anonymous diagnostics; the private journal retains the full list.
    pub entries: Vec<FileEntryResultV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileEntryResultV1 {
    pub file: String,
    pub status: String,
}
