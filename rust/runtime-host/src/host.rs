use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Deserialize;

use crate::engine::{
    EngineAdapter, EngineContextHandle, EngineContextSnapshot, EngineContextSpec, EngineEvalMode,
    EngineEvalOutcome, EngineEvalRequest, EngineJobDrain, EngineSessionHandle, EngineSessionSpec,
};
use crate::error::{RuntimeHostError, RuntimeHostResult};
use crate::protocol::{
    ArchiveStats, CapabilityMatrix, HostBootstrapSummary, HostContextFsCommand, HostFsCommand,
    HostFsResponse, HostProcessInfo, HostRuntimeBindings, HostRuntimeBootstrapModule,
    HostRuntimeBootstrapPlan, HostRuntimeBuiltinSpec, HostRuntimeCommand, HostRuntimeConsoleLevel,
    HostRuntimeContext, HostRuntimeEngineBoot, HostRuntimeEvent, HostRuntimeHttpRequest,
    HostRuntimeHttpServer, HostRuntimeHttpServerKind, HostRuntimeLoadedModule,
    HostRuntimeModuleRecord, HostRuntimeModuleSource, HostRuntimePort, HostRuntimePortProtocol,
    HostRuntimeResolvedModule, HostRuntimeResponse, HostRuntimeStdioStream, HostRuntimeTimer,
    HostRuntimeTimerKind,
    PreviewRequestHint, PreviewRequestKind, PreviewResponseDescriptor, PreviewResponseKind,
    RunPlan, RunRequest, SessionSnapshot, SessionState, WorkspaceEntrySummary,
    WorkspaceFileSummary,
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
    engine_session: EngineSessionHandle,
    vfs: VirtualFileSystem,
}

#[derive(Debug, Deserialize)]
struct PackageManifest {
    name: Option<String>,
    main: Option<String>,
    module: Option<String>,
    scripts: Option<BTreeMap<String, String>>,
    dependencies: Option<BTreeMap<String, String>>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
struct RuntimeContextRecord {
    session_id: String,
    process: HostProcessInfo,
    engine_context: EngineContextHandle,
    clock_ms: u64,
    next_port: u16,
    ports: BTreeMap<u16, RuntimePortRecord>,
    http_servers: BTreeMap<u16, RuntimeHttpServerRecord>,
    timers: BTreeMap<String, RuntimeTimerRecord>,
    events: VecDeque<HostRuntimeEvent>,
    exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeTimerRecord {
    timer_id: String,
    kind: HostRuntimeTimerKind,
    delay_ms: u64,
    due_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimePortRecord {
    port: u16,
    protocol: HostRuntimePortProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeHttpServerRecord {
    port: RuntimePortRecord,
    kind: HostRuntimeHttpServerKind,
    cwd: String,
    entrypoint: String,
}

pub struct RuntimeHostCore<E: EngineAdapter> {
    engine: E,
    sessions: BTreeMap<String, SessionRecord>,
    next_session_id: u64,
    runtime_contexts: BTreeMap<String, RuntimeContextRecord>,
    next_runtime_context_id: u64,
    next_runtime_timer_id: u64,
}

impl<E: EngineAdapter> RuntimeHostCore<E> {
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            sessions: BTreeMap::new(),
            next_session_id: 1,
            runtime_contexts: BTreeMap::new(),
            next_runtime_context_id: 1,
            next_runtime_timer_id: 1,
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
        let package_manifest = read_package_manifest(&vfs);

        let snapshot = SessionSnapshot {
            session_id: session_id.clone(),
            state: SessionState::Mounted,
            revision: 0,
            workspace_root: "/workspace".into(),
            archive,
            package_name: package_manifest
                .as_ref()
                .and_then(|manifest| manifest.name.clone())
                .or(package_name),
            capabilities: CapabilityMatrix {
                detected_react: package_manifest
                    .as_ref()
                    .is_some_and(detect_react_dependency),
                detected_vite: package_manifest
                    .as_ref()
                    .is_some_and(detect_vite_dependency),
            },
        };

        let engine_session = self
            .engine
            .boot_session(&EngineSessionSpec {
                session_id: session_id.clone(),
                workspace_root: snapshot.workspace_root.clone(),
            })
            .map_err(RuntimeHostError::EngineFailure)?;

        self.sessions.insert(
            session_id,
            SessionRecord {
                snapshot: snapshot.clone(),
                package_scripts: package_manifest
                    .as_ref()
                    .and_then(|manifest| manifest.scripts.clone())
                    .unwrap_or(package_scripts),
                engine_session,
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

    pub fn build_process_info(
        &self,
        session_id: &str,
        request: &RunRequest,
    ) -> RuntimeHostResult<HostProcessInfo> {
        let plan = self.plan_run(session_id, request)?;
        let argv = match plan.command_kind {
            crate::protocol::RunCommandKind::NodeEntrypoint => {
                let mut argv = vec![String::from("/virtual/node"), plan.entrypoint.clone()];
                argv.extend(request.args.iter().skip(1).cloned());
                argv
            }
            crate::protocol::RunCommandKind::NpmScript => {
                let mut argv = vec![
                    String::from("/virtual/node"),
                    String::from("npm"),
                    String::from("run"),
                ];
                argv.push(plan.entrypoint.clone());
                argv.extend(request.args.iter().skip(2).cloned());
                argv
            }
        };

        Ok(HostProcessInfo {
            cwd: plan.cwd,
            argv,
            env: request.env.clone(),
            exec_path: String::from("/virtual/node"),
            platform: String::from("browser"),
            entrypoint: plan.entrypoint,
            command_line: plan.command_line,
            command_kind: plan.command_kind,
        })
    }

    pub fn create_runtime_context(
        &mut self,
        session_id: &str,
        request: &RunRequest,
    ) -> RuntimeHostResult<HostRuntimeContext> {
        let process = self.build_process_info(session_id, request)?;
        let engine_session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?
            .engine_session
            .clone();
        let context_id = format!("runtime-context-{}", self.next_runtime_context_id);
        self.next_runtime_context_id += 1;
        let engine_context = self
            .engine
            .create_context(&EngineContextSpec {
                context_id: context_id.clone(),
                session_id: session_id.to_string(),
                engine_session_id: engine_session.engine_session_id,
                cwd: process.cwd.clone(),
                entrypoint: process.entrypoint.clone(),
                argv_len: process.argv.len(),
                env_count: process.env.len(),
            })
            .map_err(RuntimeHostError::EngineFailure)?;

        self.runtime_contexts.insert(
            context_id.clone(),
            RuntimeContextRecord {
                session_id: session_id.to_string(),
                process: process.clone(),
                engine_context,
                clock_ms: 0,
                next_port: 3000,
                ports: BTreeMap::new(),
                http_servers: BTreeMap::new(),
                timers: BTreeMap::new(),
                events: VecDeque::new(),
                exit_code: None,
            },
        );

        Ok(HostRuntimeContext {
            context_id,
            session_id: session_id.to_string(),
            process,
        })
    }

    pub fn describe_engine_context(
        &self,
        context_id: &str,
    ) -> RuntimeHostResult<EngineContextSnapshot> {
        let engine_context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?
            .engine_context
            .clone();

        self.engine
            .describe_context(&engine_context)
            .ok_or_else(|| {
                RuntimeHostError::EngineFailure(format!(
                    "engine context not found: {}",
                    engine_context.engine_context_id
                ))
            })
    }

    pub fn eval_engine_context(
        &mut self,
        context_id: &str,
        filename: impl Into<String>,
        source: impl Into<String>,
        as_module: bool,
    ) -> RuntimeHostResult<EngineEvalOutcome> {
        let engine_context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?
            .engine_context
            .clone();

        self.engine
            .eval(
                &engine_context,
                &EngineEvalRequest {
                    filename: filename.into(),
                    source: source.into(),
                    mode: if as_module {
                        EngineEvalMode::Module
                    } else {
                        EngineEvalMode::Script
                    },
                },
            )
            .map_err(RuntimeHostError::EngineFailure)
    }

    pub fn drain_engine_jobs(&mut self, context_id: &str) -> RuntimeHostResult<EngineJobDrain> {
        let engine_context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?
            .engine_context
            .clone();

        self.engine
            .drain_jobs(&engine_context)
            .map_err(RuntimeHostError::EngineFailure)
    }

    pub fn interrupt_engine_context(
        &mut self,
        context_id: &str,
        reason: &str,
    ) -> RuntimeHostResult<()> {
        let engine_context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?
            .engine_context
            .clone();

        self.engine
            .interrupt(&engine_context, reason)
            .map_err(RuntimeHostError::EngineFailure)
    }

    pub fn list_engine_modules(
        &self,
        context_id: &str,
    ) -> RuntimeHostResult<Vec<HostRuntimeModuleRecord>> {
        let engine_context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?
            .engine_context
            .clone();

        self.engine
            .list_modules(&engine_context)
            .map(|modules| {
                modules
                    .into_iter()
                    .map(|module| HostRuntimeModuleRecord {
                        specifier: module.specifier,
                        source_len: module.source.len(),
                    })
                    .collect()
            })
            .map_err(RuntimeHostError::EngineFailure)
    }

    pub fn read_engine_module(
        &self,
        context_id: &str,
        specifier: &str,
    ) -> RuntimeHostResult<HostRuntimeModuleSource> {
        let engine_context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?
            .engine_context
            .clone();

        self.engine
            .read_module(&engine_context, specifier)
            .map(|module| HostRuntimeModuleSource {
                specifier: module.specifier,
                source: module.source,
            })
            .map_err(RuntimeHostError::EngineFailure)
    }

    pub fn resolve_runtime_module(
        &self,
        context_id: &str,
        importer: Option<&str>,
        specifier: &str,
    ) -> RuntimeHostResult<HostRuntimeResolvedModule> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        let record = self
            .sessions
            .get(&context.session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(context.session_id.clone()))?;

        if self
            .engine
            .read_module(&context.engine_context, specifier)
            .is_ok()
        {
            return Ok(HostRuntimeResolvedModule {
                requested_specifier: specifier.to_string(),
                resolved_specifier: specifier.to_string(),
                kind: crate::protocol::HostRuntimeModuleKind::Registered,
                format: crate::protocol::HostRuntimeModuleFormat::Module,
            });
        }

        if specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/')
        {
            let base_dir = importer
                .filter(|value| value.starts_with("/workspace"))
                .map(dirname)
                .unwrap_or(&context.process.cwd);
            let requested = if specifier.starts_with('/') {
                normalize_posix_path(specifier)
            } else {
                normalize_posix_path(&format!("{base_dir}/{specifier}"))
            };
            let resolved = resolve_workspace_module(record, &requested)?;
            return Ok(HostRuntimeResolvedModule {
                requested_specifier: specifier.to_string(),
                resolved_specifier: resolved.clone(),
                kind: crate::protocol::HostRuntimeModuleKind::Workspace,
                format: detect_module_format(&resolved),
            });
        }

        let base_dir = importer
            .filter(|value| value.starts_with("/workspace"))
            .map(dirname)
            .unwrap_or(&context.process.cwd);
        let resolved = resolve_package_module(record, base_dir, specifier)?;
        Ok(HostRuntimeResolvedModule {
            requested_specifier: specifier.to_string(),
            resolved_specifier: resolved.clone(),
            kind: crate::protocol::HostRuntimeModuleKind::Workspace,
            format: detect_module_format(&resolved),
        })
    }

    pub fn load_runtime_module(
        &self,
        context_id: &str,
        resolved_specifier: &str,
    ) -> RuntimeHostResult<HostRuntimeLoadedModule> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        let record = self
            .sessions
            .get(&context.session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(context.session_id.clone()))?;

        if let Ok(module) = self
            .engine
            .read_module(&context.engine_context, resolved_specifier)
        {
            return Ok(HostRuntimeLoadedModule {
                resolved_specifier: module.specifier,
                kind: crate::protocol::HostRuntimeModuleKind::Registered,
                format: crate::protocol::HostRuntimeModuleFormat::Module,
                source: module.source,
            });
        }

        let file = record
            .vfs
            .read(resolved_specifier)
            .ok_or_else(|| RuntimeHostError::ModuleNotFound(resolved_specifier.to_string()))?;
        if !file.is_text {
            return Err(RuntimeHostError::EngineFailure(format!(
                "module source must be text: {resolved_specifier}"
            )));
        }

        Ok(HostRuntimeLoadedModule {
            resolved_specifier: resolved_specifier.to_string(),
            kind: crate::protocol::HostRuntimeModuleKind::Workspace,
            format: detect_module_format(resolved_specifier),
            source: String::from_utf8_lossy(&file.bytes).into_owned(),
        })
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

        let entry = record.vfs.write_file(&resolved, bytes, is_text)?;
        sync_session_package_manifest(record, &resolved);
        Ok(entry)
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

                let entry = record.vfs.write_file(&resolved, bytes, is_text)?;
                sync_session_package_manifest(record, &resolved);
                Ok(HostFsResponse::Entry(entry))
            }
        }
    }

    pub fn execute_context_fs_command(
        &mut self,
        context_id: &str,
        command: HostContextFsCommand,
    ) -> RuntimeHostResult<HostFsResponse> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .cloned()
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

        let fs_command = match command {
            HostContextFsCommand::Exists { path } => HostFsCommand::Exists {
                cwd: context.process.cwd.clone(),
                path,
            },
            HostContextFsCommand::Stat { path } => HostFsCommand::Stat {
                cwd: context.process.cwd.clone(),
                path,
            },
            HostContextFsCommand::ReadDir { path } => HostFsCommand::ReadDir {
                cwd: context.process.cwd.clone(),
                path,
            },
            HostContextFsCommand::ReadFile { path } => HostFsCommand::ReadFile {
                cwd: context.process.cwd.clone(),
                path,
            },
            HostContextFsCommand::CreateDirAll { path } => HostFsCommand::CreateDirAll {
                cwd: context.process.cwd.clone(),
                path,
            },
            HostContextFsCommand::WriteFile {
                path,
                bytes,
                is_text,
            } => HostFsCommand::WriteFile {
                cwd: context.process.cwd.clone(),
                path,
                bytes,
                is_text,
            },
        };

        self.execute_fs_command(&context.session_id, fs_command)
    }

    pub fn execute_runtime_command(
        &mut self,
        context_id: &str,
        command: HostRuntimeCommand,
    ) -> RuntimeHostResult<HostRuntimeResponse> {
        match command {
            HostRuntimeCommand::DescribeBindings => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .cloned()
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
                let bindings =
                    build_runtime_bindings(context_id, &context, self.engine.descriptor());

                Ok(HostRuntimeResponse::Bindings(bindings))
            }
            HostRuntimeCommand::DescribeBootstrap => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .cloned()
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
                let bindings =
                    build_runtime_bindings(context_id, &context, self.engine.descriptor());

                Ok(HostRuntimeResponse::BootstrapPlan(
                    build_runtime_bootstrap_plan(&bindings),
                ))
            }
            HostRuntimeCommand::BootEngine => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .cloned()
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
                let bindings =
                    build_runtime_bindings(context_id, &context, self.engine.descriptor());
                let plan = build_runtime_bootstrap_plan(&bindings);
                let eval = self
                    .engine
                    .bootstrap(&context.engine_context, &plan)
                    .map_err(RuntimeHostError::EngineFailure)?;
                let drained = self
                    .engine
                    .drain_jobs(&context.engine_context)
                    .map_err(RuntimeHostError::EngineFailure)?;

                Ok(HostRuntimeResponse::EngineBoot(HostRuntimeEngineBoot {
                    plan,
                    result_summary: eval.result_summary,
                    pending_jobs: eval.pending_jobs,
                    drained_jobs: drained.drained_jobs,
                }))
            }
            HostRuntimeCommand::DescribeModules => {
                let modules = self.list_engine_modules(context_id)?;
                Ok(HostRuntimeResponse::ModuleList { modules })
            }
            HostRuntimeCommand::ReadModule { specifier } => {
                let module = self.read_engine_module(context_id, &specifier)?;
                Ok(HostRuntimeResponse::ModuleSource(module))
            }
            HostRuntimeCommand::ResolveModule { specifier, importer } => {
                let module = self.resolve_runtime_module(context_id, importer.as_deref(), &specifier)?;
                Ok(HostRuntimeResponse::ModuleResolved { module })
            }
            HostRuntimeCommand::LoadModule { resolved_specifier } => {
                let module = self.load_runtime_module(context_id, &resolved_specifier)?;
                Ok(HostRuntimeResponse::ModuleLoaded { module })
            }
            HostRuntimeCommand::StdioWrite { stream, chunk } => {
                let queue_len = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    let event = match stream {
                        HostRuntimeStdioStream::Stdout => HostRuntimeEvent::Stdout { chunk },
                        HostRuntimeStdioStream::Stderr => HostRuntimeEvent::Stderr { chunk },
                    };
                    context.events.push_back(event);
                    context.events.len()
                };

                Ok(HostRuntimeResponse::EventQueued { queue_len })
            }
            HostRuntimeCommand::ConsoleEmit { level, values } => {
                let queue_len = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    let line = values.join(" ");
                    context.events.push_back(HostRuntimeEvent::Console {
                        level: level.clone(),
                        line: line.clone(),
                    });
                    context.events.push_back(match level {
                        HostRuntimeConsoleLevel::Warn | HostRuntimeConsoleLevel::Error => {
                            HostRuntimeEvent::Stderr { chunk: line }
                        }
                        HostRuntimeConsoleLevel::Log | HostRuntimeConsoleLevel::Info => {
                            HostRuntimeEvent::Stdout { chunk: line }
                        }
                    });
                    context.events.len()
                };

                Ok(HostRuntimeResponse::EventQueued { queue_len })
            }
            HostRuntimeCommand::DrainEvents => {
                let events = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    context.events.drain(..).collect::<Vec<_>>()
                };

                Ok(HostRuntimeResponse::RuntimeEvents { events })
            }
            HostRuntimeCommand::PortListen { port, protocol } => {
                let port = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    let port = allocate_runtime_port(context, port)?;
                    let port_record = RuntimePortRecord {
                        port,
                        protocol: protocol.clone(),
                    };
                    context.ports.insert(port, port_record.clone());
                    context.events.push_back(HostRuntimeEvent::PortListen {
                        port: runtime_port_view(&port_record),
                    });
                    runtime_port_view(&port_record)
                };

                Ok(HostRuntimeResponse::PortListening { port })
            }
            HostRuntimeCommand::PortClose { port } => {
                let existed = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    let existed = context.ports.remove(&port).is_some();
                    context.http_servers.remove(&port);
                    if existed {
                        context
                            .events
                            .push_back(HostRuntimeEvent::PortClose { port });
                    }
                    existed
                };

                Ok(HostRuntimeResponse::PortClosed { port, existed })
            }
            HostRuntimeCommand::PortList => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::PortList {
                    ports: context.ports.values().map(runtime_port_view).collect(),
                })
            }
            HostRuntimeCommand::HttpServePreview { port } => {
                let server = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    let port = allocate_runtime_port(context, port)?;
                    let port_record = RuntimePortRecord {
                        port,
                        protocol: HostRuntimePortProtocol::Http,
                    };
                    context.ports.insert(port, port_record.clone());
                    let server_record = RuntimeHttpServerRecord {
                        port: port_record.clone(),
                        kind: HostRuntimeHttpServerKind::Preview,
                        cwd: context.process.cwd.clone(),
                        entrypoint: context.process.entrypoint.clone(),
                    };
                    context.http_servers.insert(port, server_record.clone());
                    context.events.push_back(HostRuntimeEvent::PortListen {
                        port: runtime_port_view(&port_record),
                    });
                    runtime_http_server_view(&server_record)
                };

                Ok(HostRuntimeResponse::HttpServerListening { server })
            }
            HostRuntimeCommand::HttpCloseServer { port } => {
                let existed = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    let existed = context.http_servers.remove(&port).is_some();
                    if existed {
                        context.ports.remove(&port);
                        context
                            .events
                            .push_back(HostRuntimeEvent::PortClose { port });
                    }
                    existed
                };

                Ok(HostRuntimeResponse::HttpServerClosed { port, existed })
            }
            HostRuntimeCommand::HttpListServers => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::HttpServerList {
                    servers: context
                        .http_servers
                        .values()
                        .map(runtime_http_server_view)
                        .collect(),
                })
            }
            HostRuntimeCommand::HttpResolvePreview { request } => {
                let (session_id, server) = {
                    let context = self.runtime_contexts.get(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    let server = context
                        .http_servers
                        .get(&request.port)
                        .cloned()
                        .ok_or(RuntimeHostError::HttpServerNotFound(request.port))?;
                    (context.session_id.clone(), server)
                };
                let request_hint =
                    self.resolve_preview_request_hint(&session_id, &request.relative_path)?;
                let response_descriptor =
                    describe_preview_response(&request_hint, request.method.as_str());

                Ok(HostRuntimeResponse::PreviewRequestResolved {
                    server: runtime_http_server_view(&server),
                    port: runtime_port_view(&server.port),
                    request: runtime_http_request_view(&request),
                    request_hint,
                    response_descriptor,
                })
            }
            HostRuntimeCommand::TimerSchedule { delay_ms, repeat } => {
                let timer = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    let timer_id = format!("runtime-timer-{}", self.next_runtime_timer_id);
                    self.next_runtime_timer_id += 1;
                    let timer = RuntimeTimerRecord {
                        timer_id: timer_id.clone(),
                        kind: if repeat {
                            HostRuntimeTimerKind::Interval
                        } else {
                            HostRuntimeTimerKind::Timeout
                        },
                        delay_ms,
                        due_at_ms: context.clock_ms.saturating_add(delay_ms),
                    };
                    context.timers.insert(timer_id, timer.clone());
                    runtime_timer_view(&timer)
                };

                Ok(HostRuntimeResponse::TimerScheduled { timer })
            }
            HostRuntimeCommand::TimerClear { timer_id } => {
                let existed = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    context.timers.remove(&timer_id).is_some()
                };

                Ok(HostRuntimeResponse::TimerCleared { timer_id, existed })
            }
            HostRuntimeCommand::TimerList => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::TimerList {
                    now_ms: context.clock_ms,
                    timers: context.timers.values().map(runtime_timer_view).collect(),
                })
            }
            HostRuntimeCommand::TimerAdvance { elapsed_ms } => {
                let (now_ms, timers) = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    context.clock_ms = context.clock_ms.saturating_add(elapsed_ms);
                    let now_ms = context.clock_ms;
                    let timers = advance_runtime_timers(context, now_ms);
                    (now_ms, timers)
                };

                Ok(HostRuntimeResponse::TimerFired { now_ms, timers })
            }
            HostRuntimeCommand::ProcessInfo => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .cloned()
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::ProcessInfo(context.process))
            }
            HostRuntimeCommand::ProcessStatus => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::ProcessStatus {
                    exited: context.exit_code.is_some(),
                    exit_code: context.exit_code,
                })
            }
            HostRuntimeCommand::ProcessCwd => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::ProcessCwd {
                    cwd: context.process.cwd.clone(),
                })
            }
            HostRuntimeCommand::ProcessArgv => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::ProcessArgv {
                    argv: context.process.argv.clone(),
                })
            }
            HostRuntimeCommand::ProcessEnv => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::ProcessEnv {
                    env: context.process.env.clone(),
                })
            }
            HostRuntimeCommand::ProcessExit { code } => {
                let response = {
                    let context = self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    context.exit_code = Some(code);
                    context
                        .events
                        .push_back(HostRuntimeEvent::ProcessExit { code });
                    HostRuntimeResponse::ProcessStatus {
                        exited: true,
                        exit_code: Some(code),
                    }
                };

                Ok(response)
            }
            HostRuntimeCommand::ProcessChdir { path } => {
                let (session_id, cwd) = {
                    let context = self.runtime_contexts.get(context_id).ok_or_else(|| {
                        RuntimeHostError::RuntimeContextNotFound(context_id.into())
                    })?;
                    (context.session_id.clone(), context.process.cwd.clone())
                };
                let record = self
                    .sessions
                    .get(&session_id)
                    .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.clone()))?;
                let resolved = resolve_fs_command_path(record, &cwd, &path)?;

                match record.vfs.stat(&resolved) {
                    Some(WorkspaceEntrySummary {
                        kind: crate::protocol::WorkspaceEntryKind::Directory,
                        ..
                    }) => {
                        let context =
                            self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                                RuntimeHostError::RuntimeContextNotFound(context_id.into())
                            })?;
                        context.process.cwd = resolved.clone();
                        Ok(HostRuntimeResponse::ProcessCwd { cwd: resolved })
                    }
                    Some(_) => Err(RuntimeHostError::NotADirectory(resolved)),
                    None => Err(RuntimeHostError::DirectoryNotFound(resolved)),
                }
            }
            HostRuntimeCommand::PathResolve { segments } => {
                let context = self
                    .runtime_contexts
                    .get(context_id)
                    .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

                Ok(HostRuntimeResponse::PathValue {
                    value: resolve_runtime_path(&context.process.cwd, &segments),
                })
            }
            HostRuntimeCommand::PathJoin { segments } => Ok(HostRuntimeResponse::PathValue {
                value: join_runtime_path(&segments),
            }),
            HostRuntimeCommand::PathDirname { path } => Ok(HostRuntimeResponse::PathValue {
                value: runtime_dirname(&path),
            }),
            HostRuntimeCommand::PathBasename { path } => Ok(HostRuntimeResponse::PathValue {
                value: runtime_basename(&path),
            }),
            HostRuntimeCommand::PathExtname { path } => Ok(HostRuntimeResponse::PathValue {
                value: runtime_extname(&path),
            }),
            HostRuntimeCommand::PathNormalize { path } => Ok(HostRuntimeResponse::PathValue {
                value: normalize_runtime_path(&path),
            }),
            HostRuntimeCommand::Fs(command) => {
                let emit_workspace_change = matches!(
                    command,
                    HostContextFsCommand::CreateDirAll { .. }
                        | HostContextFsCommand::WriteFile { .. }
                );
                let response = self.execute_context_fs_command(context_id, command)?;

                if emit_workspace_change {
                    if let HostFsResponse::Entry(entry) = &response {
                        let session_id = {
                            let context =
                                self.runtime_contexts.get(context_id).ok_or_else(|| {
                                    RuntimeHostError::RuntimeContextNotFound(context_id.into())
                                })?;
                            context.session_id.clone()
                        };
                        let revision = {
                            let session = self.sessions.get_mut(&session_id).ok_or_else(|| {
                                RuntimeHostError::SessionNotFound(session_id.clone())
                            })?;
                            session.snapshot.revision += 1;
                            session.snapshot.revision
                        };
                        let context =
                            self.runtime_contexts.get_mut(context_id).ok_or_else(|| {
                                RuntimeHostError::RuntimeContextNotFound(context_id.into())
                            })?;
                        context.events.push_back(HostRuntimeEvent::WorkspaceChange {
                            entry: entry.clone(),
                            revision,
                        });
                    }
                }

                Ok(HostRuntimeResponse::Fs(response))
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
        let session = self
            .sessions
            .remove(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let context_handles = self
            .runtime_contexts
            .iter()
            .filter(|(_, context)| context.session_id == session_id)
            .map(|(_, context)| context.engine_context.clone())
            .collect::<Vec<_>>();

        for handle in &context_handles {
            self.engine.dispose_context(handle);
        }
        self.runtime_contexts
            .retain(|_, context| context.session_id != session_id);
        self.engine.dispose_session(&session.engine_session);

        Ok(())
    }

    pub fn drop_runtime_context(&mut self, context_id: &str) -> RuntimeHostResult<()> {
        self.runtime_contexts
            .remove(context_id)
            .map(|context| {
                self.engine.dispose_context(&context.engine_context);
            })
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))
    }
}

fn runtime_timer_view(timer: &RuntimeTimerRecord) -> HostRuntimeTimer {
    HostRuntimeTimer {
        timer_id: timer.timer_id.clone(),
        kind: timer.kind.clone(),
        delay_ms: timer.delay_ms,
        due_at_ms: timer.due_at_ms,
    }
}

fn build_runtime_bindings(
    context_id: &str,
    context: &RuntimeContextRecord,
    engine: crate::engine::EngineDescriptor,
) -> HostRuntimeBindings {
    HostRuntimeBindings {
        context_id: context_id.to_string(),
        engine_name: engine.name.to_string(),
        entrypoint: context.process.entrypoint.clone(),
        globals: vec![
            "console".into(),
            "process".into(),
            "Buffer".into(),
            "setTimeout".into(),
            "clearTimeout".into(),
            "__runtime".into(),
        ],
        builtins: vec![
            HostRuntimeBuiltinSpec {
                name: "process".into(),
                globals: vec!["process".into()],
                modules: vec!["process".into(), "node:process".into()],
                command_prefixes: vec!["process".into()],
            },
            HostRuntimeBuiltinSpec {
                name: "fs".into(),
                globals: Vec::new(),
                modules: vec!["fs".into(), "node:fs".into()],
                command_prefixes: vec!["fs".into()],
            },
            HostRuntimeBuiltinSpec {
                name: "path".into(),
                globals: Vec::new(),
                modules: vec!["path".into(), "node:path".into()],
                command_prefixes: vec!["path".into()],
            },
            HostRuntimeBuiltinSpec {
                name: "buffer".into(),
                globals: vec!["Buffer".into()],
                modules: vec!["buffer".into(), "node:buffer".into()],
                command_prefixes: Vec::new(),
            },
            HostRuntimeBuiltinSpec {
                name: "timers".into(),
                globals: vec!["setTimeout".into(), "clearTimeout".into()],
                modules: vec!["timers".into(), "node:timers".into()],
                command_prefixes: vec!["timers".into()],
            },
            HostRuntimeBuiltinSpec {
                name: "console".into(),
                globals: vec!["console".into()],
                modules: vec!["console".into(), "node:console".into()],
                command_prefixes: vec!["console".into()],
            },
        ],
    }
}

fn build_runtime_bootstrap_plan(bindings: &HostRuntimeBindings) -> HostRuntimeBootstrapPlan {
    let bootstrap_specifier = "runtime:bootstrap".to_string();
    let entrypoint_literal = serde_json::to_string(&bindings.entrypoint)
        .expect("entrypoint should serialize as json string");

    HostRuntimeBootstrapPlan {
        context_id: bindings.context_id.clone(),
        engine_name: bindings.engine_name.clone(),
        entrypoint: bindings.entrypoint.clone(),
        bootstrap_specifier: bootstrap_specifier.clone(),
        modules: vec![
            HostRuntimeBootstrapModule {
                specifier: "node:process".into(),
                source: r#"const runtime = globalThis.__runtime;
function invoke(kind, payload = {}) {
  if (!runtime || typeof runtime.invoke !== "function") {
    throw new Error("runtime bridge is not attached");
  }
  return runtime.invoke(kind, payload);
}
const process = {
  cwd() { return invoke("process.cwd"); },
  chdir(path) { return invoke("process.chdir", { path }); },
  exit(code = 0) { return invoke("process.exit", { code }); },
  get argv() { return invoke("process.argv"); },
  get env() { return invoke("process.env"); },
  platform: "browser",
};
export default process;
export const cwd = () => process.cwd();
export const chdir = (path) => process.chdir(path);
export const exit = (code = 0) => process.exit(code);
"#
                .into(),
            },
            HostRuntimeBootstrapModule {
                specifier: "node:fs".into(),
                source: r#"const runtime = globalThis.__runtime;
function invoke(kind, payload = {}) {
  if (!runtime || typeof runtime.invoke !== "function") {
    throw new Error("runtime bridge is not attached");
  }
  return runtime.invoke(kind, payload);
}
export const existsSync = (path) => invoke("fs.exists", { path }).exists;
export const statSync = (path) => invoke("fs.stat", { path }).entry;
export const readdirSync = (path) => invoke("fs.read-dir", { path }).entries.map((entry) => entry.path);
export const readFileSync = (path) => invoke("fs.read-file", { path });
export const mkdirSync = (path) => invoke("fs.mkdir", { path });
export const writeFileSync = (path, bytes, isText = false) =>
  invoke("fs.write-file", { path, bytes, isText });
export default {
  existsSync,
  statSync,
  readdirSync,
  readFileSync,
  mkdirSync,
  writeFileSync,
};
"#
                .into(),
            },
            HostRuntimeBootstrapModule {
                specifier: "node:path".into(),
                source: r#"const runtime = globalThis.__runtime;
function invoke(kind, payload = {}) {
  if (!runtime || typeof runtime.invoke !== "function") {
    throw new Error("runtime bridge is not attached");
  }
  return runtime.invoke(kind, payload).value;
}
export const resolve = (...segments) => invoke("path.resolve", { segments });
export const join = (...segments) => invoke("path.join", { segments });
export const dirname = (path) => invoke("path.dirname", { path });
export const basename = (path) => invoke("path.basename", { path });
export const extname = (path) => invoke("path.extname", { path });
export const normalize = (path) => invoke("path.normalize", { path });
export default { resolve, join, dirname, basename, extname, normalize };
"#
                .into(),
            },
            HostRuntimeBootstrapModule {
                specifier: "node:buffer".into(),
                source: r#"export const Buffer = Uint8Array;
export default { Buffer };
"#
                .into(),
            },
            HostRuntimeBootstrapModule {
                specifier: "node:timers".into(),
                source: r#"const runtime = globalThis.__runtime;
function invoke(kind, payload = {}) {
  if (!runtime || typeof runtime.invoke !== "function") {
    throw new Error("runtime bridge is not attached");
  }
  return runtime.invoke(kind, payload);
}
export const setTimeout = (callback, delay = 0) => invoke("timers.schedule", { delayMs: delay, repeat: false, callback });
export const clearTimeout = (timerId) => invoke("timers.clear", { timerId });
export default { setTimeout, clearTimeout };
"#
                .into(),
            },
            HostRuntimeBootstrapModule {
                specifier: "node:console".into(),
                source: r#"const runtime = globalThis.__runtime;
function emit(level, values) {
  if (!runtime || typeof runtime.invoke !== "function") {
    throw new Error("runtime bridge is not attached");
  }
  return runtime.invoke("console.emit", { level, values });
}
const consoleValue = {
  log: (...values) => emit("log", values),
  info: (...values) => emit("info", values),
  warn: (...values) => emit("warn", values),
  error: (...values) => emit("error", values),
};
export { consoleValue as console };
export default consoleValue;
"#
                .into(),
            },
            HostRuntimeBootstrapModule {
                specifier: bootstrap_specifier,
                source: format!(
                    r#"import process from "node:process";
import {{ Buffer }} from "node:buffer";
import consoleValue from "node:console";
import {{ setTimeout, clearTimeout }} from "node:timers";

globalThis.process = process;
globalThis.Buffer = Buffer;
globalThis.console = consoleValue;
globalThis.setTimeout = setTimeout;
globalThis.clearTimeout = clearTimeout;

export async function boot() {{
  return import({entrypoint_literal});
}}
"#
                ),
            },
        ],
    }
}

fn runtime_port_view(port: &RuntimePortRecord) -> HostRuntimePort {
    HostRuntimePort {
        port: port.port,
        protocol: port.protocol.clone(),
    }
}

fn runtime_http_request_view(request: &HostRuntimeHttpRequest) -> HostRuntimeHttpRequest {
    HostRuntimeHttpRequest {
        port: request.port,
        method: request.method.clone(),
        relative_path: request.relative_path.clone(),
        search: request.search.clone(),
    }
}

fn runtime_http_server_view(server: &RuntimeHttpServerRecord) -> HostRuntimeHttpServer {
    HostRuntimeHttpServer {
        port: runtime_port_view(&server.port),
        kind: server.kind.clone(),
        cwd: server.cwd.clone(),
        entrypoint: server.entrypoint.clone(),
    }
}

fn describe_preview_response(
    request_hint: &PreviewRequestHint,
    request_method: &str,
) -> PreviewResponseDescriptor {
    let method = request_method.to_ascii_uppercase();
    if method != "GET" && method != "HEAD" {
        return PreviewResponseDescriptor {
            kind: PreviewResponseKind::MethodNotAllowed,
            workspace_path: None,
            document_root: None,
            hydrate_paths: Vec::new(),
            status_code: 405,
            content_type: Some(String::from("application/json; charset=utf-8")),
            allow_methods: vec![String::from("GET"), String::from("HEAD")],
            omit_body: false,
        };
    }

    let kind = match request_hint.kind {
        PreviewRequestKind::RootDocument => PreviewResponseKind::WorkspaceDocument,
        PreviewRequestKind::RootEntry => PreviewResponseKind::AppShell,
        PreviewRequestKind::FallbackRoot => PreviewResponseKind::HostManagedFallback,
        PreviewRequestKind::RuntimeState => PreviewResponseKind::RuntimeState,
        PreviewRequestKind::WorkspaceState => PreviewResponseKind::WorkspaceState,
        PreviewRequestKind::FileIndex => PreviewResponseKind::FileIndex,
        PreviewRequestKind::DiagnosticsState => PreviewResponseKind::DiagnosticsState,
        PreviewRequestKind::RuntimeStylesheet => PreviewResponseKind::RuntimeStylesheet,
        PreviewRequestKind::WorkspaceFile => PreviewResponseKind::WorkspaceFile,
        PreviewRequestKind::WorkspaceAsset => PreviewResponseKind::WorkspaceAsset,
        PreviewRequestKind::NotFound => PreviewResponseKind::NotFound,
    };
    let status_code = if matches!(kind, PreviewResponseKind::NotFound) {
        404
    } else {
        200
    };
    let content_type = guess_preview_content_type(&kind, request_hint.workspace_path.as_deref());
    let omit_body = method == "HEAD";

    PreviewResponseDescriptor {
        kind,
        workspace_path: request_hint.workspace_path.clone(),
        document_root: request_hint.document_root.clone(),
        hydrate_paths: if omit_body {
            Vec::new()
        } else {
            request_hint.hydrate_paths.clone()
        },
        status_code,
        content_type,
        allow_methods: if omit_body || method == "GET" {
            Vec::new()
        } else {
            vec![String::from("GET"), String::from("HEAD")]
        },
        omit_body,
    }
}

fn read_package_manifest(vfs: &VirtualFileSystem) -> Option<PackageManifest> {
    let file = vfs.read("/workspace/package.json")?;
    if !file.is_text {
        return None;
    }

    serde_json::from_slice::<PackageManifest>(&file.bytes).ok()
}

fn detect_react_dependency(manifest: &PackageManifest) -> bool {
    dependency_keys(manifest).any(|name| name == "react" || name == "react-dom")
}

fn detect_vite_dependency(manifest: &PackageManifest) -> bool {
    dependency_keys(manifest).any(|name| name == "vite")
}

fn dependency_keys(manifest: &PackageManifest) -> impl Iterator<Item = &str> {
    manifest
        .dependencies
        .iter()
        .flat_map(|deps| deps.keys().map(String::as_str))
        .chain(
            manifest
                .dev_dependencies
                .iter()
                .flat_map(|deps| deps.keys().map(String::as_str)),
        )
}

fn sync_session_package_manifest(record: &mut SessionRecord, path: &str) {
    if path != "/workspace/package.json" {
        return;
    }

    let manifest = read_package_manifest(&record.vfs);
    record.package_scripts = manifest
        .as_ref()
        .and_then(|package| package.scripts.clone())
        .unwrap_or_default();
    record.snapshot.package_name = manifest.as_ref().and_then(|package| package.name.clone());
    record.snapshot.capabilities.detected_react =
        manifest.as_ref().is_some_and(detect_react_dependency);
    record.snapshot.capabilities.detected_vite =
        manifest.as_ref().is_some_and(detect_vite_dependency);
}

fn guess_preview_content_type(
    kind: &PreviewResponseKind,
    workspace_path: Option<&str>,
) -> Option<String> {
    match kind {
        PreviewResponseKind::WorkspaceDocument
        | PreviewResponseKind::AppShell
        | PreviewResponseKind::HostManagedFallback => {
            Some(String::from("text/html; charset=utf-8"))
        }
        PreviewResponseKind::RuntimeState
        | PreviewResponseKind::WorkspaceState
        | PreviewResponseKind::FileIndex
        | PreviewResponseKind::DiagnosticsState
        | PreviewResponseKind::MethodNotAllowed
        | PreviewResponseKind::NotFound => Some(String::from("application/json; charset=utf-8")),
        PreviewResponseKind::RuntimeStylesheet => Some(String::from("text/css; charset=utf-8")),
        PreviewResponseKind::WorkspaceFile | PreviewResponseKind::WorkspaceAsset => workspace_path
            .map(|path| {
                let extension = path
                    .rsplit_once('.')
                    .map(|(_, suffix)| suffix.to_ascii_lowercase());

                match extension.as_deref() {
                    Some("html") => "text/html; charset=utf-8",
                    Some("css") => "text/css; charset=utf-8",
                    Some("js") | Some("mjs") | Some("cjs") | Some("jsx") => {
                        "text/javascript; charset=utf-8"
                    }
                    Some("ts") | Some("tsx") => "text/plain; charset=utf-8",
                    Some("json") => "application/json; charset=utf-8",
                    Some("svg") => "image/svg+xml",
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("gif") => "image/gif",
                    Some("webp") => "image/webp",
                    Some("ico") => "image/x-icon",
                    Some("woff") => "font/woff",
                    Some("woff2") => "font/woff2",
                    Some("txt") => "text/plain; charset=utf-8",
                    _ => "application/octet-stream",
                }
                .to_string()
            }),
    }
}

fn allocate_runtime_port(
    context: &mut RuntimeContextRecord,
    requested: Option<u16>,
) -> RuntimeHostResult<u16> {
    if let Some(port) = requested.filter(|port| *port > 0) {
        if context.ports.contains_key(&port) {
            return Err(RuntimeHostError::PortAlreadyInUse(port));
        }
        return Ok(port);
    }

    let mut candidate = context.next_port.max(3000);
    while context.ports.contains_key(&candidate) {
        candidate = candidate.saturating_add(1);
    }
    context.next_port = candidate.saturating_add(1);
    Ok(candidate)
}

fn advance_runtime_timers(
    context: &mut RuntimeContextRecord,
    now_ms: u64,
) -> Vec<HostRuntimeTimer> {
    let due_timer_ids = context
        .timers
        .iter()
        .filter(|(_, timer)| timer.due_at_ms <= now_ms)
        .map(|(timer_id, _)| timer_id.clone())
        .collect::<Vec<_>>();

    let mut fired = Vec::new();

    for timer_id in due_timer_ids {
        let Some(timer) = context.timers.get(&timer_id).cloned() else {
            continue;
        };

        fired.push(runtime_timer_view(&timer));

        match timer.kind {
            HostRuntimeTimerKind::Timeout => {
                context.timers.remove(&timer_id);
            }
            HostRuntimeTimerKind::Interval => {
                let mut next_due_at = timer.due_at_ms;
                let step = timer.delay_ms.max(1);
                while next_due_at <= now_ms {
                    next_due_at = next_due_at.saturating_add(step);
                }

                if let Some(existing) = context.timers.get_mut(&timer_id) {
                    existing.due_at_ms = next_due_at;
                }
            }
        }
    }

    fired
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

fn resolve_workspace_module(record: &SessionRecord, requested: &str) -> RuntimeHostResult<String> {
    for candidate in workspace_module_candidates(requested) {
        if record.vfs.read(&candidate).is_some() {
            return Ok(candidate);
        }
    }

    Err(RuntimeHostError::ModuleNotFound(requested.to_string()))
}

fn resolve_package_module(
    record: &SessionRecord,
    importer_dir: &str,
    specifier: &str,
) -> RuntimeHostResult<String> {
    let (package_name, subpath) = split_package_specifier(specifier);
    for package_root in node_module_search_roots(importer_dir, &package_name) {
        let package_json_path = format!("{package_root}/package.json");
        let manifest = record
            .vfs
            .read(&package_json_path)
            .and_then(|file| serde_json::from_slice::<PackageManifest>(&file.bytes).ok());

        if let Some(subpath) = subpath.as_deref() {
            let requested = normalize_posix_path(&format!("{package_root}/{subpath}"));
            if let Ok(resolved) = resolve_workspace_module(record, &requested) {
                return Ok(resolved);
            }
        } else if let Some(manifest) = manifest {
            if let Some(entry) = manifest.module.or(manifest.main) {
                let requested = normalize_posix_path(&format!("{package_root}/{entry}"));
                if let Ok(resolved) = resolve_workspace_module(record, &requested) {
                    return Ok(resolved);
                }
            }
        }

        if let Ok(resolved) = resolve_workspace_module(record, &format!("{package_root}/index")) {
            return Ok(resolved);
        }
    }

    Err(RuntimeHostError::ModuleNotFound(specifier.to_string()))
}

fn workspace_module_candidates(requested: &str) -> Vec<String> {
    let normalized = normalize_posix_path(requested);
    [
        normalized.clone(),
        format!("{normalized}.js"),
        format!("{normalized}.mjs"),
        format!("{normalized}.cjs"),
        format!("{normalized}.ts"),
        format!("{normalized}.tsx"),
        format!("{normalized}.jsx"),
        format!("{normalized}.json"),
        format!("{normalized}/index.js"),
        format!("{normalized}/index.mjs"),
        format!("{normalized}/index.cjs"),
        format!("{normalized}/index.ts"),
        format!("{normalized}/index.tsx"),
        format!("{normalized}/index.jsx"),
        format!("{normalized}/index.json"),
    ]
    .into_iter()
    .collect()
}

fn split_package_specifier(specifier: &str) -> (String, Option<String>) {
    if let Some(stripped) = specifier.strip_prefix('@') {
        let mut parts = stripped.splitn(3, '/');
        let scope = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        let package_name = format!("@{scope}/{name}");
        let subpath = parts.next().map(ToOwned::to_owned);
        return (package_name, subpath);
    }

    let mut parts = specifier.splitn(2, '/');
    let package_name = parts.next().unwrap_or_default().to_string();
    let subpath = parts.next().map(ToOwned::to_owned);
    (package_name, subpath)
}

fn node_module_search_roots(importer_dir: &str, package_name: &str) -> Vec<String> {
    let mut roots = BTreeSet::new();
    let mut current = importer_dir.to_string();

    while current.starts_with("/workspace") {
        if current.ends_with("/node_modules") {
            roots.insert(normalize_posix_path(&format!("{current}/{package_name}")));
        } else {
            roots.insert(normalize_posix_path(&format!("{current}/node_modules/{package_name}")));
        }

        if current == "/workspace" {
            break;
        }

        current = dirname(&current).to_string();
    }

    roots.into_iter().collect()
}

fn detect_module_format(path: &str) -> crate::protocol::HostRuntimeModuleFormat {
    if path.ends_with(".cjs") {
        crate::protocol::HostRuntimeModuleFormat::CommonJs
    } else if path.ends_with(".json") {
        crate::protocol::HostRuntimeModuleFormat::Json
    } else {
        crate::protocol::HostRuntimeModuleFormat::Module
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

fn normalize_runtime_path(path: &str) -> String {
    let normalized = normalize_posix_path(path);
    if normalized.is_empty() {
        ".".into()
    } else {
        normalized
    }
}

fn join_runtime_path(segments: &[String]) -> String {
    if segments.is_empty() {
        return ".".into();
    }

    let joined = segments.iter().filter(|segment| !segment.is_empty()).fold(
        String::new(),
        |current, segment| {
            if current.is_empty() {
                segment.clone()
            } else {
                format!("{current}/{segment}")
            }
        },
    );

    if joined.is_empty() {
        ".".into()
    } else {
        normalize_runtime_path(&joined)
    }
}

fn resolve_runtime_path(cwd: &str, segments: &[String]) -> String {
    let mut resolved = cwd.to_string();

    for segment in segments {
        if segment.is_empty() {
            continue;
        }

        if segment.starts_with('/') {
            resolved = segment.clone();
        } else if resolved == "/" {
            resolved = format!("/{segment}");
        } else {
            resolved = format!("{resolved}/{segment}");
        }
    }

    normalize_runtime_path(&resolved)
}

fn runtime_dirname(path: &str) -> String {
    let normalized = normalize_runtime_path(path);

    if normalized == "/" {
        return "/".into();
    }

    let trimmed = normalized.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".into(),
        Some(index) => trimmed[..index].to_string(),
        None => ".".into(),
    }
}

fn runtime_basename(path: &str) -> String {
    let normalized = normalize_runtime_path(path);

    if normalized == "/" {
        return "/".into();
    }

    normalized
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(".")
        .to_string()
}

fn runtime_extname(path: &str) -> String {
    let basename = runtime_basename(path);

    if basename == "/" || basename == "." || basename == ".." {
        return String::new();
    }

    match basename.rfind('.') {
        Some(0) | None => String::new(),
        Some(index) => basename[index..].to_string(),
    }
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
