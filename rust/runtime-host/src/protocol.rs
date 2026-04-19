use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveStats {
    pub file_name: String,
    pub file_count: usize,
    pub directory_count: usize,
    pub root_prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityMatrix {
    pub detected_react: bool,
    pub detected_vite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Booting,
    Mounted,
    Running,
    Stopped,
    Errored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub state: SessionState,
    pub workspace_root: String,
    pub archive: ArchiveStats,
    pub package_name: Option<String>,
    pub capabilities: CapabilityMatrix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl RunRequest {
    pub fn new(cwd: impl Into<String>, command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            cwd: cwd.into(),
            command: command.into(),
            args,
            env: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub cwd: String,
    pub entrypoint: String,
    pub command_line: String,
    pub env_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBootstrapSummary {
    pub engine_name: String,
    pub supports_interrupts: bool,
    pub supports_module_loader: bool,
    pub workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileSummary {
    pub path: String,
    pub size: usize,
    pub is_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewRequestKind {
    RootDocument,
    RootEntry,
    FallbackRoot,
    RuntimeState,
    WorkspaceState,
    FileIndex,
    DiagnosticsState,
    RuntimeStylesheet,
    WorkspaceFile,
    WorkspaceAsset,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRequestHint {
    pub kind: PreviewRequestKind,
    pub workspace_path: Option<String>,
    pub document_root: Option<String>,
    pub hydrate_paths: Vec<String>,
}
