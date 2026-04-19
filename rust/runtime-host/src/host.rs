use std::collections::{BTreeMap, BTreeSet};

use crate::engine::EngineAdapter;
use crate::error::{RuntimeHostError, RuntimeHostResult};
use crate::protocol::{
    ArchiveStats, CapabilityMatrix, HostBootstrapSummary, PreviewAssetHint, PreviewRequestHint,
    PreviewRequestKind, PreviewRootHint, PreviewRootKind, RunPlan, RunRequest, SessionSnapshot,
    SessionState, WorkspaceFileSummary,
};
use crate::vfs::{VirtualFile, VirtualFileSystem};

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

#[derive(Debug)]
struct SessionRecord {
    snapshot: SessionSnapshot,
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
        files: Vec<VirtualFile>,
    ) -> RuntimeHostResult<SessionSnapshot> {
        let session_id = format!("rust-session-{}", self.next_session_id);
        self.next_session_id += 1;

        self.create_session_with_id(session_id, archive, package_name, files)
    }

    pub fn create_session_with_id(
        &mut self,
        session_id: String,
        archive: ArchiveStats,
        package_name: Option<String>,
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
                vfs,
            },
        );

        Ok(snapshot)
    }

    pub fn plan_run(&self, session_id: &str, request: &RunRequest) -> RuntimeHostResult<RunPlan> {
        if !self.sessions.contains_key(session_id) {
            return Err(RuntimeHostError::SessionNotFound(session_id.into()));
        }

        Ok(self.engine.plan_run(request))
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

    pub fn resolve_preview_hydration_paths(
        &self,
        session_id: &str,
        relative_path: &str,
    ) -> RuntimeHostResult<Vec<String>> {
        Ok(self
            .resolve_preview_request_hint(session_id, relative_path)?
            .hydrate_paths)
    }

    pub fn resolve_preview_root_hint(
        &self,
        session_id: &str,
    ) -> RuntimeHostResult<PreviewRootHint> {
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

    pub fn resolve_preview_asset_hint(
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
