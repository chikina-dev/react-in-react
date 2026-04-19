use std::collections::{BTreeMap, BTreeSet};

use crate::engine::EngineAdapter;
use crate::error::{RuntimeHostError, RuntimeHostResult};
use crate::protocol::{
    ArchiveStats, CapabilityMatrix, HostBootstrapSummary, HostFsCommand, HostFsResponse,
    PreviewRequestHint, PreviewRequestKind, RunPlan, RunRequest, SessionSnapshot, SessionState,
    WorkspaceEntrySummary, WorkspaceFileSummary,
};
use crate::vfs::{VirtualFile, VirtualFileSystem, normalize_posix_path};

const PREVIEW_DOCUMENT_CANDIDATES: [&str; 4] = [
    "/workspace/index.html",
    "/workspace/dist/index.html",
    "/workspace/build/index.html",
    "/workspace/public/index.html",
];

const PREVIEW_APP_ENTRY_CANDIDATES: [&str; 8] = [
    "/workspace/src/main.tsx",
    "/workspace/src/main.jsx",
    "/workspace/src/main.ts",
    "/workspace/src/main.js",
    "/workspace/src/index.tsx",
    "/workspace/src/index.jsx",
    "/workspace/src/index.ts",
    "/workspace/src/index.js",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreviewRootKind {
    WorkspaceDocument,
    SourceEntry,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewRootHint {
    kind: PreviewRootKind,
    path: Option<String>,
    root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewAssetHint {
    workspace_path: Option<String>,
    document_root: Option<String>,
}

#[derive(Debug)]
struct SessionRecord {
    snapshot: SessionSnapshot,
    package_scripts: BTreeMap<String, String>,
    vfs: VirtualFileSystem,
}

pub struct RuntimeHostCore<E: EngineAdapter> {
    engine: E,
    sessions: BTreeMap<String, SessionRecord>,
    next_session_id: u64,
}

impl<E: EngineAdapter> RuntimeHostCore<E> {
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            sessions: BTreeMap::new(),
            next_session_id: 1,
        }
    }

    pub fn boot_summary(&self) -> HostBootstrapSummary {
        let descriptor = self.engine.descriptor();

        HostBootstrapSummary {
            engine_name: descriptor.name.to_string(),
            supports_interrupts: descriptor.supports_interrupts,
            supports_module_loader: descriptor.supports_module_loader,
            workspace_root: "/workspace".into(),
        }
    }

    pub fn create_session(
        &mut self,
        archive: ArchiveStats,
        package_name: Option<String>,
        package_scripts: BTreeMap<String, String>,
        files: Vec<VirtualFile>,
    ) -> RuntimeHostResult<SessionSnapshot> {
        let session_id = format!("rust-session-{}", self.next_session_id);
        self.next_session_id += 1;

        self.create_session_with_id(session_id, archive, package_name, package_scripts, files)
    }

    pub fn create_session_with_id(
        &mut self,
        session_id: String,
        archive: ArchiveStats,
        package_name: Option<String>,
        package_scripts: BTreeMap<String, String>,
        files: Vec<VirtualFile>,
    ) -> RuntimeHostResult<SessionSnapshot> {
        self.next_session_id = self.next_session_id.max(
            session_id
                .strip_prefix("rust-session-")
                .and_then(|value| value.parse::<u64>().ok())
                .map(|value| value + 1)
                .unwrap_or(self.next_session_id),
        );

        let mut vfs = VirtualFileSystem::new("/workspace");
        vfs.mount_files(files)?;

        let snapshot = SessionSnapshot {
            session_id: session_id.clone(),
            state: SessionState::Mounted,
            workspace_root: "/workspace".into(),
            archive,
            package_name,
            capabilities: CapabilityMatrix {
                detected_react: vfs.read("/workspace/package.json").is_some(),
                detected_vite: vfs.file_count() > 0,
            },
        };

        self.sessions.insert(
            session_id,
            SessionRecord {
                snapshot: snapshot.clone(),
                package_scripts,
                vfs,
            },
        );

        Ok(snapshot)
    }

    pub fn plan_run(&self, session_id: &str, request: &RunRequest) -> RuntimeHostResult<RunPlan> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;

        let cwd = resolve_run_cwd(record, &request.cwd)?;
        let command_line = std::iter::once(request.command.as_str())
            .chain(request.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");

        if request.command == "npm" && request.args.first().map(String::as_str) == Some("run") {
            let script_name = request
                .args
                .get(1)
                .ok_or(RuntimeHostError::ScriptNotFound("<missing>".into()))?;
            let script = record
                .package_scripts
                .get(script_name)
                .cloned()
                .ok_or_else(|| RuntimeHostError::ScriptNotFound(script_name.clone()))?;

            return Ok(RunPlan {
                cwd,
                entrypoint: script_name.clone(),
                command_line,
                env_count: request.env.len(),
                command_kind: crate::protocol::RunCommandKind::NpmScript,
                resolved_script: Some(script),
            });
        }

        if request.command == "node" {
            let entrypoint = resolve_node_entrypoint(record, &cwd, request.args.first())?;

            return Ok(RunPlan {
                cwd,
                entrypoint,
                command_line,
                env_count: request.env.len(),
                command_kind: crate::protocol::RunCommandKind::NodeEntrypoint,
                resolved_script: None,
            });
        }

        let engine_plan = self.engine.plan_run(request);

        if request.command.is_empty() {
            return Err(RuntimeHostError::UnsupportedCommand("<empty>".into()));
        }

        Err(RuntimeHostError::UnsupportedCommand(
            if command_line.is_empty() {
                engine_plan.entrypoint
            } else {
                command_line
            },
        ))
    }

    pub fn session_file_system(&self, session_id: &str) -> Option<&VirtualFileSystem> {
        self.sessions.get(session_id).map(|record| &record.vfs)
    }

    pub fn session_snapshot(&self, session_id: &str) -> Option<&SessionSnapshot> {
        self.sessions.get(session_id).map(|record| &record.snapshot)
    }

    pub fn workspace_file_summaries(
        &self,
        session_id: &str,
    ) -> RuntimeHostResult<Vec<WorkspaceFileSummary>> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;

        Ok(record
            .vfs
            .files()
            .map(|file| WorkspaceFileSummary {
                path: file.path.clone(),
                size: file.bytes.len(),
                is_text: file.is_text,
            })
            .collect())
    }

    pub fn read_workspace_file(
        &self,
        session_id: &str,
        path: &str,
    ) -> RuntimeHostResult<VirtualFile> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;

        record
            .vfs
            .read(path)
            .cloned()
            .ok_or_else(|| RuntimeHostError::FileNotFound(path.into()))
    }

    pub fn stat_workspace_path(
        &self,
        session_id: &str,
        path: &str,
    ) -> RuntimeHostResult<WorkspaceEntrySummary> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let resolved = resolve_workspace_path(record, path);

        record
            .vfs
            .stat(&resolved)
            .ok_or_else(|| RuntimeHostError::FileNotFound(resolved))
    }

    pub fn read_workspace_directory(
        &self,
        session_id: &str,
        path: &str,
    ) -> RuntimeHostResult<Vec<WorkspaceEntrySummary>> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let resolved = resolve_workspace_path(record, path);

        record.vfs.read_dir(&resolved)
    }

    pub fn create_workspace_directory(
        &mut self,
        session_id: &str,
        path: &str,
    ) -> RuntimeHostResult<WorkspaceEntrySummary> {
        let record = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let resolved = resolve_workspace_path(record, path);

        record.vfs.create_dir_all(&resolved)?;
        record
            .vfs
            .stat(&resolved)
            .ok_or_else(|| RuntimeHostError::DirectoryNotFound(resolved))
    }

    pub fn write_workspace_file(
        &mut self,
        session_id: &str,
        path: &str,
        bytes: Vec<u8>,
        is_text: bool,
    ) -> RuntimeHostResult<WorkspaceEntrySummary> {
        let record = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let resolved = resolve_workspace_path(record, path);

        record.vfs.write_file(&resolved, bytes, is_text)
    }

    pub fn execute_fs_command(
        &mut self,
        session_id: &str,
        command: HostFsCommand,
    ) -> RuntimeHostResult<HostFsResponse> {
        match command {
            HostFsCommand::Exists { cwd, path } => {
                let record = self
                    .sessions
                    .get(session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
                let resolved = resolve_fs_command_path(record, &cwd, &path)?;

                Ok(HostFsResponse::Exists {
                    path: resolved.clone(),
                    exists: record.vfs.exists(&resolved),
                })
            }
            HostFsCommand::Stat { cwd, path } => {
                let record = self
                    .sessions
                    .get(session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
                let resolved = resolve_fs_command_path(record, &cwd, &path)?;

                record
                    .vfs
                    .stat(&resolved)
                    .map(HostFsResponse::Entry)
                    .ok_or(RuntimeHostError::FileNotFound(resolved))
            }
            HostFsCommand::ReadDir { cwd, path } => {
                let record = self
                    .sessions
                    .get(session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
                let resolved = resolve_fs_command_path(record, &cwd, &path)?;

                record
                    .vfs
                    .read_dir(&resolved)
                    .map(HostFsResponse::DirectoryEntries)
            }
            HostFsCommand::ReadFile { cwd, path } => {
                let record = self
                    .sessions
                    .get(session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
                let resolved = resolve_fs_command_path(record, &cwd, &path)?;
                let file = record
                    .vfs
                    .read(&resolved)
                    .cloned()
                    .ok_or_else(|| RuntimeHostError::FileNotFound(resolved.clone()))?;
                let text_content = if file.is_text {
                    Some(String::from_utf8_lossy(&file.bytes).into_owned())
                } else {
                    None
                };

                Ok(HostFsResponse::File {
                    path: file.path,
                    size: file.bytes.len(),
                    is_text: file.is_text,
                    text_content,
                    bytes: file.bytes,
                })
            }
            HostFsCommand::CreateDirAll { cwd, path } => {
                let record = self
                    .sessions
                    .get_mut(session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
                let resolved = resolve_fs_command_path(record, &cwd, &path)?;

                record.vfs.create_dir_all(&resolved)?;
                record
                    .vfs
                    .stat(&resolved)
                    .map(HostFsResponse::Entry)
                    .ok_or(RuntimeHostError::DirectoryNotFound(resolved))
            }
            HostFsCommand::WriteFile {
                cwd,
                path,
                bytes,
                is_text,
            } => {
                let record = self
                    .sessions
                    .get_mut(session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
                let resolved = resolve_fs_command_path(record, &cwd, &path)?;

                record
                    .vfs
                    .write_file(&resolved, bytes, is_text)
                    .map(HostFsResponse::Entry)
            }
        }
    }

    fn resolve_preview_root_hint(&self, session_id: &str) -> RuntimeHostResult<PreviewRootHint> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;

        for candidate in PREVIEW_DOCUMENT_CANDIDATES {
            if let Some(file) = record.vfs.read(candidate) {
                if file.is_text && file.path.ends_with(".html") {
                    return Ok(PreviewRootHint {
                        kind: PreviewRootKind::WorkspaceDocument,
                        path: Some(file.path.clone()),
                        root: Some(dirname(candidate).to_string()),
                    });
                }
            }
        }

        for candidate in PREVIEW_APP_ENTRY_CANDIDATES {
            if record.vfs.read(candidate).is_some() {
                return Ok(PreviewRootHint {
                    kind: PreviewRootKind::SourceEntry,
                    path: Some(candidate.to_string()),
                    root: None,
                });
            }
        }

        Ok(PreviewRootHint {
            kind: PreviewRootKind::Fallback,
            path: None,
            root: None,
        })
    }

    fn resolve_preview_asset_hint(
        &self,
        session_id: &str,
        relative_path: &str,
    ) -> RuntimeHostResult<PreviewAssetHint> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let root_hint = self.resolve_preview_root_hint(session_id)?;

        if relative_path.starts_with("/__") || relative_path == "/assets/runtime.css" {
            return Ok(PreviewAssetHint {
                workspace_path: None,
                document_root: None,
            });
        }

        if relative_path.starts_with("/files/") {
            let workspace_path = decode_workspace_path(relative_path);
            return Ok(PreviewAssetHint {
                workspace_path: record
                    .vfs
                    .read(&workspace_path)
                    .map(|file| file.path.clone()),
                document_root: Some("/workspace".into()),
            });
        }

        let document_root = root_hint.root.unwrap_or_else(|| "/workspace".into());
        let normalized = normalize_workspace_asset_path(relative_path);

        let mut candidates = vec![
            format!("{document_root}{normalized}"),
            format!("/workspace{normalized}"),
        ];

        if normalized.ends_with('/') {
            candidates.push(format!("{document_root}{normalized}index.html"));
            candidates.push(format!("/workspace{normalized}index.html"));
        }

        for candidate in candidates {
            if let Some(file) = record.vfs.read(&candidate) {
                return Ok(PreviewAssetHint {
                    workspace_path: Some(file.path.clone()),
                    document_root: Some(document_root),
                });
            }
        }

        Ok(PreviewAssetHint {
            workspace_path: None,
            document_root: Some(document_root),
        })
    }

    pub fn resolve_preview_request_hint(
        &self,
        session_id: &str,
        relative_path: &str,
    ) -> RuntimeHostResult<PreviewRequestHint> {
        match relative_path {
            "/" | "/index.html" => match self.resolve_preview_root_hint(session_id)? {
                PreviewRootHint {
                    kind: PreviewRootKind::WorkspaceDocument,
                    path: Some(path),
                    root,
                } => Ok(PreviewRequestHint {
                    kind: PreviewRequestKind::RootDocument,
                    workspace_path: Some(path.clone()),
                    document_root: root,
                    hydrate_paths: collect_preview_hydrate_paths(
                        self.sessions.get(session_id).expect("session exists"),
                        Some(path.as_str()),
                    ),
                }),
                PreviewRootHint {
                    kind: PreviewRootKind::SourceEntry,
                    path: Some(path),
                    ..
                } => Ok(PreviewRequestHint {
                    kind: PreviewRequestKind::RootEntry,
                    workspace_path: Some(path.clone()),
                    document_root: None,
                    hydrate_paths: collect_preview_hydrate_paths(
                        self.sessions.get(session_id).expect("session exists"),
                        Some(path.as_str()),
                    ),
                }),
                _ => Ok(PreviewRequestHint {
                    kind: PreviewRequestKind::FallbackRoot,
                    workspace_path: None,
                    document_root: None,
                    hydrate_paths: collect_preview_hydrate_paths(
                        self.sessions.get(session_id).expect("session exists"),
                        None,
                    ),
                }),
            },
            "/__runtime.json" => Ok(PreviewRequestHint {
                kind: PreviewRequestKind::RuntimeState,
                workspace_path: None,
                document_root: None,
                hydrate_paths: Vec::new(),
            }),
            "/__workspace.json" => Ok(PreviewRequestHint {
                kind: PreviewRequestKind::WorkspaceState,
                workspace_path: None,
                document_root: None,
                hydrate_paths: Vec::new(),
            }),
            "/__files.json" => Ok(PreviewRequestHint {
                kind: PreviewRequestKind::FileIndex,
                workspace_path: None,
                document_root: None,
                hydrate_paths: Vec::new(),
            }),
            "/__diagnostics.json" => Ok(PreviewRequestHint {
                kind: PreviewRequestKind::DiagnosticsState,
                workspace_path: None,
                document_root: None,
                hydrate_paths: Vec::new(),
            }),
            "/assets/runtime.css" => Ok(PreviewRequestHint {
                kind: PreviewRequestKind::RuntimeStylesheet,
                workspace_path: None,
                document_root: None,
                hydrate_paths: Vec::new(),
            }),
            path if path.starts_with("/files/") => {
                let workspace_path = decode_workspace_path(path);
                let record = self
                    .sessions
                    .get(session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;

                Ok(PreviewRequestHint {
                    kind: if record.vfs.read(&workspace_path).is_some() {
                        PreviewRequestKind::WorkspaceFile
                    } else {
                        PreviewRequestKind::NotFound
                    },
                    workspace_path: record
                        .vfs
                        .read(&workspace_path)
                        .map(|file| file.path.clone()),
                    document_root: Some("/workspace".into()),
                    hydrate_paths: collect_preview_hydrate_paths(
                        record,
                        Some(workspace_path.as_str()),
                    ),
                })
            }
            path => {
                let asset_hint = self.resolve_preview_asset_hint(session_id, path)?;
                let record = self
                    .sessions
                    .get(session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
                let workspace_path = asset_hint.workspace_path.clone();

                Ok(PreviewRequestHint {
                    kind: if workspace_path.is_some() {
                        PreviewRequestKind::WorkspaceAsset
                    } else {
                        PreviewRequestKind::NotFound
                    },
                    workspace_path,
                    document_root: asset_hint.document_root,
                    hydrate_paths: collect_preview_hydrate_paths(
                        record,
                        asset_hint.workspace_path.as_deref(),
                    ),
                })
            }
        }
    }

    pub fn stop_session(&mut self, session_id: &str) -> RuntimeHostResult<()> {
        self.sessions
            .remove(session_id)
            .map(|_| ())
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))
    }
}

fn resolve_run_cwd(record: &SessionRecord, cwd: &str) -> RuntimeHostResult<String> {
    let normalized = resolve_workspace_path(record, cwd);

    if normalized == record.snapshot.workspace_root
        || normalized.starts_with(&format!("{}/", record.snapshot.workspace_root))
    {
        if !record.vfs.exists(&normalized) {
            return Err(RuntimeHostError::DirectoryNotFound(normalized));
        }

        if !record.vfs.is_dir(&normalized) {
            return Err(RuntimeHostError::NotADirectory(normalized));
        }

        return Ok(normalized);
    }

    Err(RuntimeHostError::InvalidWorkingDirectory(normalized))
}

fn resolve_workspace_path(record: &SessionRecord, path: &str) -> String {
    if path.is_empty() {
        record.snapshot.workspace_root.clone()
    } else if path.starts_with('/') {
        normalize_posix_path(path)
    } else {
        normalize_posix_path(&format!("{}/{}", record.snapshot.workspace_root, path))
    }
}

fn resolve_fs_command_path(
    record: &SessionRecord,
    cwd: &str,
    path: &str,
) -> RuntimeHostResult<String> {
    let resolved_cwd = resolve_run_cwd(
        record,
        if cwd.is_empty() {
            &record.snapshot.workspace_root
        } else {
            cwd
        },
    )?;
    let resolved = if path.is_empty() {
        resolved_cwd
    } else if path.starts_with('/') {
        normalize_posix_path(path)
    } else {
        normalize_posix_path(&format!("{resolved_cwd}/{path}"))
    };

    if resolved == record.snapshot.workspace_root
        || resolved.starts_with(&format!("{}/", record.snapshot.workspace_root))
    {
        Ok(resolved)
    } else {
        Err(RuntimeHostError::InvalidWorkspacePath(resolved))
    }
}

fn resolve_node_entrypoint(
    record: &SessionRecord,
    cwd: &str,
    entrypoint: Option<&String>,
) -> RuntimeHostResult<String> {
    let entrypoint = entrypoint.ok_or(RuntimeHostError::NodeEntrypointRequired)?;
    let requested = if entrypoint.starts_with('/') {
        normalize_posix_path(entrypoint)
    } else {
        normalize_posix_path(&format!("{cwd}/{entrypoint}"))
    };

    let candidates = [
        requested.clone(),
        format!("{requested}.js"),
        format!("{requested}.mjs"),
        format!("{requested}.cjs"),
        format!("{requested}.ts"),
        format!("{requested}.tsx"),
        format!("{requested}.jsx"),
        format!("{requested}/index.js"),
        format!("{requested}/index.ts"),
        format!("{requested}/index.tsx"),
    ];

    for candidate in candidates {
        if record.vfs.read(&candidate).is_some() {
            return Ok(candidate);
        }
    }

    Err(RuntimeHostError::EntrypointNotFound(requested))
}

fn dirname(path: &str) -> &str {
    let normalized = path.trim_end_matches('/');

    match normalized.rfind('/') {
        Some(index) if index > 0 => &normalized[..index],
        _ => "/workspace",
    }
}

fn collect_preview_hydrate_paths(
    record: &SessionRecord,
    workspace_path: Option<&str>,
) -> Vec<String> {
    let mut paths = BTreeSet::new();

    for file in record.vfs.files() {
        if file.path.ends_with("/package.json") {
            paths.insert(file.path.clone());
        }
    }

    if let Some(path) = workspace_path {
        paths.insert(path.to_string());
    }

    paths.into_iter().collect()
}

fn normalize_workspace_asset_path(relative_path: &str) -> String {
    let normalized = if relative_path.starts_with('/') {
        relative_path.to_string()
    } else {
        format!("/{relative_path}")
    };

    normalized.replace("//", "/")
}

fn decode_workspace_path(relative_path: &str) -> String {
    let suffix = relative_path
        .strip_prefix("/files")
        .unwrap_or(relative_path);
    format!("/workspace{}", decode_percent_path(suffix))
}

fn decode_percent_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = bytes[index + 1] as char;
            let low = bytes[index + 2] as char;

            if let (Some(high), Some(low)) = (hex_value(high), hex_value(low)) {
                output.push((high << 4) | low);
                index += 3;
                continue;
            }
        }

        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(input: char) -> Option<u8> {
    match input {
        '0'..='9' => Some((input as u8) - b'0'),
        'a'..='f' => Some((input as u8) - b'a' + 10),
        'A'..='F' => Some((input as u8) - b'A' + 10),
        _ => None,
    }
}
