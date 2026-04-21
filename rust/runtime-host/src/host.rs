use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::engine::{
    EngineAdapter, EngineBootstrapBridge, EngineBridgeSnapshot, EngineContextHandle,
    EngineContextSnapshot, EngineContextSpec, EngineEvalMode, EngineEvalOutcome, EngineEvalRequest,
    EngineJobDrain, EngineSessionHandle, EngineSessionSpec,
};
use crate::error::{RuntimeHostError, RuntimeHostResult};
use crate::protocol::{
    ArchiveStats, CapabilityMatrix, HostBootstrapSummary, HostContextFsCommand, HostFsCommand,
    HostFsResponse, HostProcessInfo, HostRuntimeBindings, HostRuntimeBootstrapModule,
    HostRuntimeBootstrapPlan, HostRuntimeBuiltinSpec, HostRuntimeCommand, HostRuntimeConsoleLevel,
    HostRuntimeContext, HostRuntimeEngineBoot, HostRuntimeEvent, HostRuntimeHttpRequest,
    HostRuntimeHttpServer, HostRuntimeHttpServerKind, HostRuntimeIdleReport,
    HostRuntimeLaunchReport, HostRuntimePreviewLaunchReport, HostRuntimePreviewRequestReport,
    HostRuntimeShutdownReport, HostRuntimeStartupReport, HostRuntimeLoadedModule,
    HostRuntimeModuleImportPlan, HostRuntimeModuleLoaderPlan, HostRuntimeModuleRecord, HostRuntimeModuleSource,
    HostRuntimePort, HostRuntimePortProtocol, HostRuntimeResolvedModule, HostRuntimeResponse,
    HostRuntimeStdioStream, HostRuntimeTimer, HostRuntimeTimerKind, PreviewRequestHint,
    PreviewRequestKind, PreviewResponseDescriptor, PreviewResponseKind, RunPlan, RunRequest,
    SessionSnapshot, SessionState, WorkspaceEntrySummary, WorkspaceFilePayload,
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
    exports: Option<JsonValue>,
    imports: Option<JsonValue>,
    browser: Option<JsonValue>,
    scripts: Option<BTreeMap<String, String>>,
    dependencies: Option<BTreeMap<String, String>>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PackageExportResolution {
    Missing,
    Blocked,
    Target(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserMappingResolution {
    NotMapped,
    Blocked,
    Target(String),
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

struct RuntimeModuleLoaderBridge<'a, E: EngineAdapter> {
    engine: &'a E,
    record: &'a SessionRecord,
    context: &'a RuntimeContextRecord,
}

impl<'a, E: EngineAdapter> RuntimeModuleLoaderBridge<'a, E> {
    fn describe(&self, context_id: &str) -> RuntimeHostResult<HostRuntimeModuleLoaderPlan> {
        let entry_module = HostRuntimeResolvedModule {
            requested_specifier: self.context.process.entrypoint.clone(),
            resolved_specifier: self.context.process.entrypoint.clone(),
            kind: crate::protocol::HostRuntimeModuleKind::Workspace,
            format: detect_module_format(&self.context.process.entrypoint),
        };
        let registered_specifiers = self
            .engine
            .list_modules(&self.context.engine_context)
            .map_err(RuntimeHostError::EngineFailure)?
            .into_iter()
            .map(|module| module.specifier)
            .collect::<Vec<_>>();

        Ok(HostRuntimeModuleLoaderPlan {
            context_id: context_id.to_string(),
            engine_name: self.engine.descriptor().name.to_string(),
            cwd: self.context.process.cwd.clone(),
            entrypoint: self.context.process.entrypoint.clone(),
            workspace_root: self.record.snapshot.workspace_root.clone(),
            entry_module,
            registered_specifiers,
            node_module_search_roots: node_module_directory_roots(&self.context.process.cwd),
        })
    }

    fn resolve(
        &self,
        specifier: &str,
        importer: Option<&str>,
    ) -> RuntimeHostResult<HostRuntimeResolvedModule> {
        if self
            .engine
            .read_module(&self.context.engine_context, specifier)
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
                .unwrap_or(&self.context.process.cwd);
            let requested = if specifier.starts_with('/') {
                normalize_posix_path(specifier)
            } else {
                normalize_posix_path(&format!("{base_dir}/{specifier}"))
            };
            let requested = match resolve_package_relative_browser_path(
                self.record,
                importer,
                &requested,
                specifier,
            )? {
                BrowserMappingResolution::Target(mapped) => mapped,
                BrowserMappingResolution::Blocked => {
                    return Err(RuntimeHostError::ModuleNotFound(specifier.to_string()));
                }
                BrowserMappingResolution::NotMapped => requested,
            };
            let resolved = resolve_workspace_module(self.record, &requested)?;
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
            .unwrap_or(&self.context.process.cwd);
        if specifier.starts_with('#') {
            let resolved = resolve_package_import_module(self.record, base_dir, specifier)?;
            return Ok(HostRuntimeResolvedModule {
                requested_specifier: specifier.to_string(),
                resolved_specifier: resolved.clone(),
                kind: crate::protocol::HostRuntimeModuleKind::Workspace,
                format: detect_module_format(&resolved),
            });
        }
        if let Some(resolved) = resolve_package_self_module(self.record, base_dir, specifier)? {
            return Ok(HostRuntimeResolvedModule {
                requested_specifier: specifier.to_string(),
                resolved_specifier: resolved.clone(),
                kind: crate::protocol::HostRuntimeModuleKind::Workspace,
                format: detect_module_format(&resolved),
            });
        }
        let resolved = resolve_package_module(self.record, base_dir, specifier)?;
        Ok(HostRuntimeResolvedModule {
            requested_specifier: specifier.to_string(),
            resolved_specifier: resolved.clone(),
            kind: crate::protocol::HostRuntimeModuleKind::Workspace,
            format: detect_module_format(&resolved),
        })
    }

    fn load(&self, resolved_specifier: &str) -> RuntimeHostResult<HostRuntimeLoadedModule> {
        if let Ok(module) = self
            .engine
            .read_module(&self.context.engine_context, resolved_specifier)
        {
            return Ok(HostRuntimeLoadedModule {
                resolved_specifier: module.specifier,
                kind: crate::protocol::HostRuntimeModuleKind::Registered,
                format: crate::protocol::HostRuntimeModuleFormat::Module,
                source: module.source,
            });
        }

        let file = self
            .record
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

        let outcome = self
            .engine
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
            .map_err(RuntimeHostError::EngineFailure)?;
        self.sync_engine_bridge_state(context_id)?;
        Ok(outcome)
    }

    pub fn drain_engine_jobs(&mut self, context_id: &str) -> RuntimeHostResult<EngineJobDrain> {
        let engine_context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?
            .engine_context
            .clone();

        let drain = self
            .engine
            .drain_jobs(&engine_context)
            .map_err(RuntimeHostError::EngineFailure)?;
        self.sync_engine_bridge_state(context_id)?;
        Ok(drain)
    }

    pub fn run_runtime_until_idle(
        &mut self,
        context_id: &str,
        max_turns: usize,
    ) -> RuntimeHostResult<HostRuntimeIdleReport> {
        let max_turns = max_turns.max(1);
        let mut turns = 0usize;
        let mut drained_jobs = 0usize;
        let mut fired_timers = 0usize;
        let mut reached_turn_limit = false;

        loop {
            if turns >= max_turns {
                reached_turn_limit = true;
                break;
            }

            let snapshot = self.describe_engine_context(context_id)?;
            if snapshot.pending_jobs > 0 {
                let drain = self.drain_engine_jobs(context_id)?;
                drained_jobs = drained_jobs.saturating_add(drain.drained_jobs);
                turns = turns.saturating_add(1);

                if drain.drained_jobs == 0 && drain.pending_jobs > 0 {
                    reached_turn_limit = true;
                    break;
                }

                continue;
            }

            let Some(elapsed_ms) = self.next_runtime_timer_elapsed(context_id)? else {
                break;
            };
            let (_, timers, timer_drains) = self.advance_runtime_timers(context_id, elapsed_ms)?;
            fired_timers = fired_timers.saturating_add(timers.len());
            drained_jobs = drained_jobs.saturating_add(timer_drains.drained_jobs);
            turns = turns.saturating_add(1);
        }

        let snapshot = self.describe_engine_context(context_id)?;
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

        Ok(HostRuntimeIdleReport {
            turns,
            drained_jobs,
            fired_timers,
            now_ms: context.clock_ms,
            pending_jobs: snapshot.pending_jobs,
            pending_timers: context.timers.len(),
            exited: context.exit_code.is_some(),
            exit_code: context.exit_code,
            reached_turn_limit,
        })
    }

    fn runtime_process_status(&self, context_id: &str) -> RuntimeHostResult<(bool, Option<i32>)> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        Ok((context.exit_code.is_some(), context.exit_code))
    }

    fn drain_runtime_events(&mut self, context_id: &str) -> RuntimeHostResult<Vec<HostRuntimeEvent>> {
        let context = self
            .runtime_contexts
            .get_mut(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        Ok(context.events.drain(..).collect())
    }

    fn boot_runtime_engine(&mut self, context_id: &str) -> RuntimeHostResult<HostRuntimeEngineBoot> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .cloned()
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        let session = self
            .sessions
            .get(&context.session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(context.session_id.clone()))?;
        let bindings = build_runtime_bindings(context_id, &context, self.engine.descriptor());
        let plan = build_runtime_bootstrap_plan(&bindings);
        let loader_plan = self.describe_runtime_module_loader(context_id)?;
        let import_plans = self.collect_runtime_boot_import_graph(context_id, &plan, &loader_plan)?;
        let bridge = build_engine_bootstrap_bridge(session, &context);
        let eval = self
            .engine
            .bootstrap(
                &context.engine_context,
                &plan,
                &loader_plan,
                &import_plans,
                &bridge,
            )
            .map_err(RuntimeHostError::EngineFailure)?;
        let drained = self
            .engine
            .drain_jobs(&context.engine_context)
            .map_err(RuntimeHostError::EngineFailure)?;
        self.sync_engine_bridge_state(context_id)?;

        Ok(HostRuntimeEngineBoot {
            plan,
            loader_plan,
            result_summary: eval.result_summary,
            pending_jobs: eval.pending_jobs,
            drained_jobs: drained.drained_jobs,
        })
    }

    fn run_runtime_startup(
        &mut self,
        context_id: &str,
        max_turns: usize,
    ) -> RuntimeHostResult<HostRuntimeStartupReport> {
        let boot = self.boot_runtime_engine(context_id)?;
        let entry_import_plan = self.prepare_runtime_module_import(
            context_id,
            &boot.loader_plan.entry_module.requested_specifier,
            None,
        )?;
        let idle = self.run_runtime_until_idle(context_id, max_turns)?;
        let (exited, exit_code) = self.runtime_process_status(context_id)?;

        Ok(HostRuntimeStartupReport {
            boot,
            entry_import_plan,
            idle,
            exited,
            exit_code,
        })
    }

    fn launch_runtime_preview(
        &mut self,
        context_id: &str,
        max_turns: usize,
        port: Option<u16>,
    ) -> RuntimeHostResult<HostRuntimePreviewLaunchReport> {
        let startup = self.run_runtime_startup(context_id, max_turns)?;

        if startup.exited {
            return Ok(HostRuntimePreviewLaunchReport {
                startup,
                server: None,
                port: None,
                root_request: None,
                root_request_hint: None,
                root_response_descriptor: None,
            });
        }

        let server = match self.execute_runtime_command(
            context_id,
            HostRuntimeCommand::HttpServePreview { port },
        )? {
            HostRuntimeResponse::HttpServerListening { server } => server,
            other => {
                return Err(RuntimeHostError::EngineFailure(format!(
                    "launch preview expected http server listening response, got {other:?}",
                )));
            }
        };

        let request = HostRuntimeHttpRequest {
            port: server.port.port,
            method: String::from("GET"),
            relative_path: String::from("/"),
            search: String::new(),
        };

        let (resolved_server, resolved_port, root_request_hint, root_response_descriptor) =
            match self.execute_runtime_command(
                context_id,
                HostRuntimeCommand::HttpResolvePreview {
                    request: request.clone(),
                },
            )? {
                HostRuntimeResponse::PreviewRequestResolved {
                    server,
                    port,
                    request: _,
                    request_hint,
                    response_descriptor,
                } => (server, port, request_hint, response_descriptor),
                other => {
                    return Err(RuntimeHostError::EngineFailure(format!(
                        "launch preview expected preview request resolved response, got {other:?}",
                    )));
                }
            };

        Ok(HostRuntimePreviewLaunchReport {
            startup,
            server: Some(resolved_server),
            port: Some(resolved_port),
            root_request: Some(request),
            root_request_hint: Some(root_request_hint),
            root_response_descriptor: Some(root_response_descriptor),
        })
    }

    pub fn launch_runtime(
        &mut self,
        session_id: &str,
        request: &RunRequest,
        max_turns: usize,
        port: Option<u16>,
    ) -> RuntimeHostResult<HostRuntimeLaunchReport> {
        let boot_summary = self.boot_summary();
        let run_plan = self.plan_run(session_id, request)?;
        let capabilities = self
            .sessions
            .get(session_id)
            .map(|record| record.snapshot.capabilities.clone())
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.to_string()))?;
        let runtime_context = self.create_runtime_context(session_id, request)?;
        let context_id = runtime_context.context_id.clone();
        let engine_context = self.describe_engine_context(&context_id)?;
        let runtime_context_record = self
            .runtime_contexts
            .get(&context_id)
            .cloned()
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.clone()))?;
        let bindings = build_runtime_bindings(&context_id, &runtime_context_record, self.engine.descriptor());
        let bootstrap_plan = build_runtime_bootstrap_plan(&bindings);
        let preview_launch = self.launch_runtime_preview(&context_id, max_turns, port)?;
        let startup_logs = build_runtime_startup_logs(
            &run_plan,
            &context_id,
            &capabilities,
            &runtime_context_record,
            &boot_summary,
            &engine_context,
            &bindings,
            &bootstrap_plan,
        );
        let events = self.drain_runtime_events(&context_id)?;

        Ok(HostRuntimeLaunchReport {
            boot_summary,
            run_plan,
            runtime_context,
            engine_context,
            bindings,
            bootstrap_plan,
            preview_launch,
            startup_logs,
            events,
        })
    }

    fn resolve_runtime_preview_request(
        &self,
        context_id: &str,
        request: HostRuntimeHttpRequest,
    ) -> RuntimeHostResult<HostRuntimePreviewRequestReport> {
        let (session_id, server) = {
            let context = self
                .runtime_contexts
                .get(context_id)
                .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
            let server = context
                .http_servers
                .get(&request.port)
                .cloned()
                .ok_or(RuntimeHostError::HttpServerNotFound(request.port))?;
            (context.session_id.clone(), server)
        };
        let request_hint = self.resolve_preview_request_hint(&session_id, &request.relative_path)?;
        let response_descriptor = describe_preview_response(&request_hint, request.method.as_str());
        let hydration_paths = if !response_descriptor.hydrate_paths.is_empty() {
            response_descriptor.hydrate_paths.clone()
        } else {
            request_hint.hydrate_paths.clone()
        };
        let hydrated_files = {
            let record = self
                .sessions
                .get(&session_id)
                .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.clone()))?;
            hydration_paths
                .iter()
                .filter_map(|path| record.vfs.read(path))
                .map(|file| WorkspaceFilePayload {
                    path: file.path.clone(),
                    size: file.bytes.len(),
                    is_text: file.is_text,
                    text_content: if file.is_text {
                        Some(String::from_utf8_lossy(&file.bytes).into_owned())
                    } else {
                        None
                    },
                    bytes: file.bytes.clone(),
                })
                .collect()
        };

        Ok(HostRuntimePreviewRequestReport {
            server: runtime_http_server_view(&server),
            port: runtime_port_view(&server.port),
            request,
            request_hint,
            response_descriptor,
            hydration_paths,
            hydrated_files,
        })
    }

    fn shutdown_runtime_context(
        &mut self,
        context_id: &str,
        code: i32,
    ) -> RuntimeHostResult<HostRuntimeShutdownReport> {
        let mut context = self
            .runtime_contexts
            .remove(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

        let closed_servers = context
            .http_servers
            .values()
            .map(runtime_http_server_view)
            .collect::<Vec<_>>();
        let closed_ports = context
            .ports
            .values()
            .map(runtime_port_view)
            .collect::<Vec<_>>();
        let final_exit_code = context.exit_code.unwrap_or(code);
        context.exit_code = Some(final_exit_code);

        for port in context.ports.keys().copied().collect::<Vec<_>>() {
            context
                .events
                .push_back(HostRuntimeEvent::PortClose { port });
        }
        if !context
            .events
            .iter()
            .any(|event| matches!(event, HostRuntimeEvent::ProcessExit { .. }))
        {
            context.events.push_back(HostRuntimeEvent::ProcessExit {
                code: final_exit_code,
            });
        }

        self.engine.dispose_context(&context.engine_context);

        Ok(HostRuntimeShutdownReport {
            context_id: context_id.to_string(),
            session_id: context.session_id,
            exit_code: final_exit_code,
            closed_ports,
            closed_servers,
            events: context.events.drain(..).collect(),
        })
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

    fn runtime_module_loader(
        &self,
        context_id: &str,
    ) -> RuntimeHostResult<RuntimeModuleLoaderBridge<'_, E>> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        let record = self
            .sessions
            .get(&context.session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(context.session_id.clone()))?;

        Ok(RuntimeModuleLoaderBridge {
            engine: &self.engine,
            record,
            context,
        })
    }

    pub fn resolve_runtime_module(
        &self,
        context_id: &str,
        importer: Option<&str>,
        specifier: &str,
    ) -> RuntimeHostResult<HostRuntimeResolvedModule> {
        self.runtime_module_loader(context_id)?
            .resolve(specifier, importer)
    }

    pub fn load_runtime_module(
        &self,
        context_id: &str,
        resolved_specifier: &str,
    ) -> RuntimeHostResult<HostRuntimeLoadedModule> {
        self.runtime_module_loader(context_id)?
            .load(resolved_specifier)
    }

    pub fn describe_runtime_module_loader(
        &self,
        context_id: &str,
    ) -> RuntimeHostResult<HostRuntimeModuleLoaderPlan> {
        self.runtime_module_loader(context_id)?.describe(context_id)
    }

    pub fn prepare_runtime_module_import(
        &self,
        context_id: &str,
        specifier: &str,
        importer: Option<&str>,
    ) -> RuntimeHostResult<HostRuntimeModuleImportPlan> {
        let resolved_module = self.resolve_runtime_module(context_id, importer, specifier)?;
        let loaded_module =
            self.load_runtime_module(context_id, &resolved_module.resolved_specifier)?;

        Ok(HostRuntimeModuleImportPlan {
            request_specifier: specifier.to_string(),
            importer: importer.map(ToOwned::to_owned),
            resolved_module,
            loaded_module,
        })
    }

    fn collect_runtime_boot_import_graph(
        &self,
        context_id: &str,
        plan: &HostRuntimeBootstrapPlan,
        loader_plan: &HostRuntimeModuleLoaderPlan,
    ) -> RuntimeHostResult<Vec<HostRuntimeModuleImportPlan>> {
        let entry_module =
            self.load_runtime_module(context_id, &loader_plan.entry_module.resolved_specifier)?;
        let mut imports = Vec::new();
        let mut queue = VecDeque::from([HostRuntimeModuleImportPlan {
            request_specifier: loader_plan.entry_module.requested_specifier.clone(),
            importer: Some(plan.bootstrap_specifier.clone()),
            resolved_module: loader_plan.entry_module.clone(),
            loaded_module: entry_module,
        }]);
        let mut visited = BTreeSet::new();

        while let Some(import_plan) = queue.pop_front() {
            if !visited.insert(import_plan.resolved_module.resolved_specifier.clone()) {
                continue;
            }

            let importer = import_plan.resolved_module.resolved_specifier.clone();
            let loaded_module = import_plan.loaded_module.clone();
            imports.push(import_plan);

            for specifier in
                collect_module_dependency_specifiers(&loaded_module.source, &loaded_module.format)
            {
                let child = if let Some(module) = plan
                    .modules
                    .iter()
                    .find(|module| module.specifier == specifier)
                {
                    HostRuntimeModuleImportPlan {
                        request_specifier: specifier.clone(),
                        importer: Some(importer.clone()),
                        resolved_module: HostRuntimeResolvedModule {
                            requested_specifier: specifier.clone(),
                            resolved_specifier: module.specifier.clone(),
                            kind: crate::protocol::HostRuntimeModuleKind::Registered,
                            format: crate::protocol::HostRuntimeModuleFormat::Module,
                        },
                        loaded_module: HostRuntimeLoadedModule {
                            resolved_specifier: module.specifier.clone(),
                            kind: crate::protocol::HostRuntimeModuleKind::Registered,
                            format: crate::protocol::HostRuntimeModuleFormat::Module,
                            source: module.source.clone(),
                        },
                    }
                } else {
                    self.prepare_runtime_module_import(context_id, &specifier, Some(&importer))?
                };
                if !visited.contains(&child.resolved_module.resolved_specifier) {
                    queue.push_back(child);
                }
            }
        }

        Ok(imports)
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
            HostRuntimeCommand::DescribeModuleLoader => Ok(HostRuntimeResponse::ModuleLoaderPlan(
                self.describe_runtime_module_loader(context_id)?,
            )),
            HostRuntimeCommand::BootEngine => {
                Ok(HostRuntimeResponse::EngineBoot(self.boot_runtime_engine(context_id)?))
            }
            HostRuntimeCommand::Startup { max_turns } => Ok(
                HostRuntimeResponse::StartupReport(
                    self.run_runtime_startup(context_id, max_turns)?,
                ),
            ),
            HostRuntimeCommand::LaunchPreview { max_turns, port } => Ok(
                HostRuntimeResponse::PreviewLaunchReport(
                    self.launch_runtime_preview(context_id, max_turns, port)?,
                ),
            ),
            HostRuntimeCommand::PreviewRequest { request } => Ok(
                HostRuntimeResponse::PreviewRequestReport(
                    self.resolve_runtime_preview_request(context_id, request)?,
                ),
            ),
            HostRuntimeCommand::Shutdown { code } => Ok(HostRuntimeResponse::ShutdownReport(
                self.shutdown_runtime_context(context_id, code)?,
            )),
            HostRuntimeCommand::RunUntilIdle { max_turns } => Ok(HostRuntimeResponse::IdleReport(
                self.run_runtime_until_idle(context_id, max_turns)?,
            )),
            HostRuntimeCommand::DescribeModules => {
                let modules = self.list_engine_modules(context_id)?;
                Ok(HostRuntimeResponse::ModuleList { modules })
            }
            HostRuntimeCommand::ReadModule { specifier } => {
                let module = self.read_engine_module(context_id, &specifier)?;
                Ok(HostRuntimeResponse::ModuleSource(module))
            }
            HostRuntimeCommand::PrepareModuleImport {
                specifier,
                importer,
            } => {
                let plan = self.prepare_runtime_module_import(
                    context_id,
                    &specifier,
                    importer.as_deref(),
                )?;
                Ok(HostRuntimeResponse::ModuleImportPlan { plan })
            }
            HostRuntimeCommand::ResolveModule {
                specifier,
                importer,
            } => {
                let module =
                    self.resolve_runtime_module(context_id, importer.as_deref(), &specifier)?;
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
                let events = self.drain_runtime_events(context_id)?;
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
                let (now_ms, timers, _) = self.advance_runtime_timers(context_id, elapsed_ms)?;

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
                let (exited, exit_code) = self.runtime_process_status(context_id)?;
                Ok(HostRuntimeResponse::ProcessStatus { exited, exit_code })
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

    fn next_runtime_timer_elapsed(&self, context_id: &str) -> RuntimeHostResult<Option<u64>> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        let Some(next_due_at_ms) = context.timers.values().map(|timer| timer.due_at_ms).min()
        else {
            return Ok(None);
        };

        Ok(Some(next_due_at_ms.saturating_sub(context.clock_ms)))
    }

    fn advance_runtime_timers(
        &mut self,
        context_id: &str,
        elapsed_ms: u64,
    ) -> RuntimeHostResult<(u64, Vec<HostRuntimeTimer>, EngineJobDrain)> {
        let (engine_context, now_ms, timers) = {
            let context = self
                .runtime_contexts
                .get_mut(context_id)
                .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
            context.clock_ms = context.clock_ms.saturating_add(elapsed_ms);
            let now_ms = context.clock_ms;
            let timers = advance_runtime_timers(context, now_ms);
            (context.engine_context.clone(), now_ms, timers)
        };
        let timer_ids = timers
            .iter()
            .map(|timer| timer.timer_id.clone())
            .collect::<Vec<_>>();
        self.engine
            .fire_timers(&engine_context, now_ms, &timer_ids)
            .map_err(RuntimeHostError::EngineFailure)?;
        let drain = self
            .engine
            .drain_jobs(&engine_context)
            .map_err(RuntimeHostError::EngineFailure)?;
        self.sync_engine_bridge_state(context_id)?;

        Ok((now_ms, timers, drain))
    }

    fn sync_engine_bridge_state(&mut self, context_id: &str) -> RuntimeHostResult<()> {
        let (session_id, engine_context) = {
            let context = self
                .runtime_contexts
                .get(context_id)
                .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
            (context.session_id.clone(), context.engine_context.clone())
        };
        let Some(snapshot) = self
            .engine
            .take_bridge_snapshot(&engine_context)
            .map_err(RuntimeHostError::EngineFailure)?
        else {
            return Ok(());
        };

        let (revision, changed_entries) = {
            let session = self
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.clone()))?;
            apply_engine_bridge_snapshot(session, &snapshot)
        };

        let context = self
            .runtime_contexts
            .get_mut(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        context.process.cwd = snapshot.cwd;
        context.timers = snapshot
            .timers
            .into_iter()
            .map(|timer| {
                (
                    timer.timer_id.clone(),
                    RuntimeTimerRecord {
                        timer_id: timer.timer_id,
                        kind: timer.kind,
                        delay_ms: timer.delay_ms,
                        due_at_ms: timer.due_at_ms,
                    },
                )
            })
            .collect();
        if let Some(code) = snapshot.exit_code {
            context.exit_code = Some(code);
        }
        for event in snapshot.events {
            context.events.push_back(event);
        }
        if let Some(revision) = revision {
            for entry in changed_entries {
                context
                    .events
                    .push_back(HostRuntimeEvent::WorkspaceChange { entry, revision });
            }
        }

        Ok(())
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

fn build_runtime_startup_logs(
    run_plan: &RunPlan,
    context_id: &str,
    capabilities: &CapabilityMatrix,
    context: &RuntimeContextRecord,
    boot_summary: &HostBootstrapSummary,
    engine_context: &crate::engine::EngineContextSnapshot,
    bindings: &HostRuntimeBindings,
    bootstrap_plan: &HostRuntimeBootstrapPlan,
) -> Vec<String> {
    let mut logs = vec![format!(
        "[host] engine={} interrupts={} module-loader={}",
        boot_summary.engine_name, boot_summary.supports_interrupts, boot_summary.supports_module_loader
    )];

    logs.push(format!(
        "[plan] cwd={} entry={} env={}",
        run_plan.cwd, run_plan.entrypoint, run_plan.env_count
    ));
    logs.push(format!(
        "[process] exec={} cwd={} argv={}",
        context.process.exec_path,
        context.process.cwd,
        context.process.argv.join(" ")
    ));
    logs.push(format!(
        "[engine-context] state={} pending-jobs={} entry={}",
        match engine_context.state {
            crate::engine::EngineContextState::Booted => "booted",
            crate::engine::EngineContextState::Ready => "ready",
            crate::engine::EngineContextState::Disposed => "disposed",
        },
        engine_context.pending_jobs,
        engine_context.entrypoint,
    ));
    logs.push(format!(
        "[bindings] globals={} builtins={}",
        bindings.globals.join(","),
        bindings
            .builtins
            .iter()
            .map(|builtin| builtin.name.as_str())
            .collect::<Vec<_>>()
            .join(","),
    ));
    logs.push(format!(
        "[bootstrap] bootstrap={} modules={}",
        bootstrap_plan.bootstrap_specifier,
        bootstrap_plan
            .modules
            .iter()
            .map(|module| module.specifier.as_str())
            .collect::<Vec<_>>()
            .join(","),
    ));
    logs.push(format!("[context] id={}", context_id));
    logs.push(format!(
        "[detect] react={} vite={}",
        capabilities.detected_react, capabilities.detected_vite
    ));

    logs
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
            "setInterval".into(),
            "clearInterval".into(),
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
                globals: vec![
                    "setTimeout".into(),
                    "clearTimeout".into(),
                    "setInterval".into(),
                    "clearInterval".into(),
                ],
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

fn build_engine_bootstrap_bridge(
    session: &SessionRecord,
    context: &RuntimeContextRecord,
) -> EngineBootstrapBridge {
    EngineBootstrapBridge {
        cwd: context.process.cwd.clone(),
        argv: context.process.argv.clone(),
        env: context.process.env.clone(),
        vfs: session.vfs.clone(),
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
if (!runtime.__timerState) {
  runtime.__timerState = {
    nextCallbackId: 1,
    callbackIdsByTimerId: new Map(),
    callbacks: new Map(),
  };
}
if (typeof runtime.fireTimer !== "function") {
  runtime.fireTimer = (callbackId, repeat = false) => {
    const callback = runtime.__timerState.callbacks.get(callbackId);
    if (typeof callback === "function") {
      callback();
    }
    if (!repeat) {
      runtime.__timerState.callbacks.delete(callbackId);
    }
    return runtime.__timerState.callbacks.has(callbackId);
  };
}
function invoke(kind, payload = {}) {
  if (!runtime || typeof runtime.invoke !== "function") {
    throw new Error("runtime bridge is not attached");
  }
  return runtime.invoke(kind, payload);
}
export const setTimeout = (callback, delay = 0) => {
  const callbackId = `runtime-callback-${runtime.__timerState.nextCallbackId++}`;
  runtime.__timerState.callbacks.set(callbackId, callback);
  const timerId = invoke("timers.schedule", {
    delayMs: delay,
    repeat: false,
    callback,
    callbackId,
  });
  runtime.__timerState.callbackIdsByTimerId.set(timerId, callbackId);
  return timerId;
};
export const setInterval = (callback, delay = 0) => {
  const callbackId = `runtime-callback-${runtime.__timerState.nextCallbackId++}`;
  runtime.__timerState.callbacks.set(callbackId, callback);
  const timerId = invoke("timers.schedule", {
    delayMs: delay,
    repeat: true,
    callback,
    callbackId,
  });
  runtime.__timerState.callbackIdsByTimerId.set(timerId, callbackId);
  return timerId;
};
export const clearTimeout = (timerId) => {
  const callbackId = runtime.__timerState.callbackIdsByTimerId.get(timerId);
  if (callbackId) {
    runtime.__timerState.callbackIdsByTimerId.delete(timerId);
    runtime.__timerState.callbacks.delete(callbackId);
  }
  return invoke("timers.clear", { timerId });
};
export const clearInterval = clearTimeout;
export default { setTimeout, clearTimeout, setInterval, clearInterval };
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
	import {{ setTimeout, clearTimeout, setInterval, clearInterval }} from "node:timers";

globalThis.process = process;
	globalThis.Buffer = Buffer;
	globalThis.console = consoleValue;
	globalThis.setTimeout = setTimeout;
	globalThis.clearTimeout = clearTimeout;
	globalThis.setInterval = setInterval;
	globalThis.clearInterval = clearInterval;
	
	const entryPromise = import({entrypoint_literal});
	
	export async function boot() {{
	  return entryPromise;
	}}
	
	export {{ entryPromise }};
	export default entryPromise;
	"#
	                ),
	            },
        ],
    }
}

fn collect_module_dependency_specifiers(
    source: &str,
    format: &crate::protocol::HostRuntimeModuleFormat,
) -> Vec<String> {
    let mut specifiers = Vec::new();
    match format {
        crate::protocol::HostRuntimeModuleFormat::Module => {
            for marker in [
                " from \"",
                " from '",
                "import(\"",
                "import('",
                "export * from \"",
                "export * from '",
            ] {
                collect_string_literals_after_marker(source, marker, &mut specifiers);
            }
            for marker in ["import \"", "import '"] {
                collect_line_prefixed_imports(source, marker, &mut specifiers);
            }
        }
        crate::protocol::HostRuntimeModuleFormat::CommonJs => {
            for marker in ["require(\"", "require('"] {
                collect_string_literals_after_marker(source, marker, &mut specifiers);
            }
        }
        crate::protocol::HostRuntimeModuleFormat::Json => {}
    }
    specifiers.sort();
    specifiers.dedup();
    specifiers
}

fn collect_string_literals_after_marker(source: &str, marker: &str, output: &mut Vec<String>) {
    let mut search_start = 0usize;
    while let Some(offset) = source[search_start..].find(marker) {
        let start = search_start + offset + marker.len();
        let quote = marker.as_bytes()[marker.len() - 1] as char;
        if let Some(end_offset) = source[start..].find(quote) {
            let candidate = &source[start..start + end_offset];
            if !candidate.is_empty() {
                output.push(candidate.to_string());
            }
            search_start = start + end_offset + 1;
        } else {
            break;
        }
    }
}

fn collect_line_prefixed_imports(source: &str, marker: &str, output: &mut Vec<String>) {
    for line in source.lines().map(str::trim_start) {
        if let Some(rest) = line.strip_prefix(marker) {
            let quote = marker.as_bytes()[marker.len() - 1] as char;
            if let Some(end) = rest.find(quote) {
                let candidate = &rest[..end];
                if !candidate.is_empty() {
                    output.push(candidate.to_string());
                }
            }
        }
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

fn apply_engine_bridge_snapshot(
    session: &mut SessionRecord,
    snapshot: &EngineBridgeSnapshot,
) -> (Option<u64>, Vec<WorkspaceEntrySummary>) {
    let changed_entries = diff_workspace_entries(&session.vfs, &snapshot.vfs);
    session.vfs = snapshot.vfs.clone();
    session.snapshot.archive.file_count = session.vfs.file_count();
    session.snapshot.archive.directory_count = session.vfs.directory_count();
    sync_session_package_manifest(session, "/workspace/package.json");

    if changed_entries.is_empty() {
        return (None, changed_entries);
    }

    session.snapshot.revision += 1;
    (Some(session.snapshot.revision), changed_entries)
}

fn diff_workspace_entries(
    previous: &VirtualFileSystem,
    next: &VirtualFileSystem,
) -> Vec<WorkspaceEntrySummary> {
    let mut changed = BTreeMap::new();

    for directory in next.directories() {
        if !previous.is_dir(directory) {
            if let Some(entry) = next.stat(directory) {
                changed.insert(entry.path.clone(), entry);
            }
        }
    }

    for file in next.files() {
        let has_changed = match previous.read(&file.path) {
            Some(existing) => existing.is_text != file.is_text || existing.bytes != file.bytes,
            None => true,
        };
        if has_changed {
            if let Some(entry) = next.stat(&file.path) {
                changed.insert(entry.path.clone(), entry);
            }
        }
    }

    changed.into_values().collect()
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
                let step = timer.delay_ms.max(1);
                let mut next_due_at = timer.due_at_ms.saturating_add(step);
                while next_due_at <= now_ms {
                    fired.push(runtime_timer_view(&timer));
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
        if let Ok(resolved) = resolve_package_from_root(
            record,
            importer_dir,
            &package_root,
            manifest.as_ref(),
            subpath.as_deref(),
            specifier,
        ) {
            return Ok(resolved);
        }
    }

    Err(RuntimeHostError::ModuleNotFound(specifier.to_string()))
}

fn resolve_package_import_module(
    record: &SessionRecord,
    importer_dir: &str,
    specifier: &str,
) -> RuntimeHostResult<String> {
    let Some(package_root) = resolve_nearest_package_root(record, importer_dir) else {
        return Err(RuntimeHostError::ModuleNotFound(specifier.to_string()));
    };
    let Some(manifest) = read_package_manifest_at(record, &package_root) else {
        return Err(RuntimeHostError::ModuleNotFound(specifier.to_string()));
    };
    let Some(imports_field) = manifest.imports.as_ref() else {
        return Err(RuntimeHostError::ModuleNotFound(specifier.to_string()));
    };

    match resolve_package_export_target(imports_field, specifier) {
        PackageExportResolution::Target(target) => {
            resolve_package_export_specifier(record, importer_dir, &package_root, &target)
        }
        PackageExportResolution::Blocked | PackageExportResolution::Missing => {
            Err(RuntimeHostError::ModuleNotFound(specifier.to_string()))
        }
    }
}

fn resolve_package_self_module(
    record: &SessionRecord,
    importer_dir: &str,
    specifier: &str,
) -> RuntimeHostResult<Option<String>> {
    let Some(package_root) = resolve_nearest_package_root(record, importer_dir) else {
        return Ok(None);
    };
    let Some(manifest) = read_package_manifest_at(record, &package_root) else {
        return Ok(None);
    };
    let Some(package_name) = manifest.name.as_deref() else {
        return Ok(None);
    };

    if specifier != package_name && !specifier.starts_with(&format!("{package_name}/")) {
        return Ok(None);
    }

    let remainder = specifier
        .strip_prefix(package_name)
        .unwrap_or_default()
        .trim_start_matches('/');
    let resolved = resolve_package_from_root(
        record,
        importer_dir,
        &package_root,
        Some(&manifest),
        if remainder.is_empty() {
            None
        } else {
            Some(remainder)
        },
        specifier,
    )?;
    Ok(Some(resolved))
}

fn resolve_package_from_root(
    record: &SessionRecord,
    importer_dir: &str,
    package_root: &str,
    manifest: Option<&PackageManifest>,
    remainder: Option<&str>,
    requested_specifier: &str,
) -> RuntimeHostResult<String> {
    if let Some(manifest) = manifest {
        if let Some(exports_field) = manifest.exports.as_ref() {
            let export_subpath = remainder
                .map(|path| format!("./{path}"))
                .unwrap_or_else(|| ".".to_string());
            match resolve_package_export_target(exports_field, &export_subpath) {
                PackageExportResolution::Target(target) => {
                    return resolve_package_export_specifier(
                        record,
                        importer_dir,
                        package_root,
                        &target,
                    );
                }
                PackageExportResolution::Blocked | PackageExportResolution::Missing => {
                    return Err(RuntimeHostError::ModuleNotFound(
                        requested_specifier.to_string(),
                    ));
                }
            }
        }

        if remainder.is_none() {
            if let Some(browser_entry) = manifest.browser.as_ref().and_then(JsonValue::as_str) {
                return resolve_package_export_specifier(
                    record,
                    importer_dir,
                    package_root,
                    browser_entry,
                );
            }
        } else if let Some(remainder) = remainder {
            match resolve_package_browser_subpath(manifest, package_root, remainder) {
                BrowserMappingResolution::Target(target) => {
                    return resolve_workspace_module(record, &target);
                }
                BrowserMappingResolution::Blocked => {
                    return Err(RuntimeHostError::ModuleNotFound(
                        requested_specifier.to_string(),
                    ));
                }
                BrowserMappingResolution::NotMapped => {}
            }
        }
    }

    if let Some(remainder) = remainder {
        let requested = normalize_posix_path(&format!("{package_root}/{remainder}"));
        if let Ok(resolved) = resolve_workspace_module(record, &requested) {
            return Ok(resolved);
        }
    } else if let Some(manifest) = manifest {
        if let Some(entry) = manifest.module.as_ref().or(manifest.main.as_ref()) {
            let requested = match resolve_legacy_browser_entry(manifest, package_root, entry) {
                BrowserMappingResolution::Target(target) => target,
                BrowserMappingResolution::Blocked => {
                    return Err(RuntimeHostError::ModuleNotFound(
                        requested_specifier.to_string(),
                    ));
                }
                BrowserMappingResolution::NotMapped => {
                    normalize_posix_path(&format!("{package_root}/{entry}"))
                }
            };
            if let Ok(resolved) = resolve_workspace_module(record, &requested) {
                return Ok(resolved);
            }
        }
    }

    if let Ok(resolved) = resolve_workspace_module(record, &format!("{package_root}/index")) {
        return Ok(resolved);
    }

    Err(RuntimeHostError::ModuleNotFound(
        requested_specifier.to_string(),
    ))
}

fn resolve_package_export_specifier(
    record: &SessionRecord,
    importer_dir: &str,
    package_root: &str,
    target: &str,
) -> RuntimeHostResult<String> {
    if target.starts_with('.') {
        return resolve_workspace_module(
            record,
            &normalize_posix_path(&format!("{package_root}/{target}")),
        );
    }

    if target.starts_with('/') {
        return resolve_workspace_module(record, target);
    }

    resolve_package_module(record, importer_dir, target)
}

fn resolve_package_relative_browser_path(
    record: &SessionRecord,
    importer: Option<&str>,
    requested_path: &str,
    requested_specifier: &str,
) -> RuntimeHostResult<BrowserMappingResolution> {
    let Some(importer_path) = importer.filter(|value| value.starts_with("/workspace")) else {
        return Ok(BrowserMappingResolution::NotMapped);
    };
    let Some(package_root) = resolve_nearest_package_root(record, dirname(importer_path)) else {
        return Ok(BrowserMappingResolution::NotMapped);
    };
    if !requested_path.starts_with(&format!("{package_root}/")) && requested_path != package_root {
        return Ok(BrowserMappingResolution::NotMapped);
    }
    let Some(manifest) = read_package_manifest_at(record, &package_root) else {
        return Ok(BrowserMappingResolution::NotMapped);
    };

    Ok(resolve_package_browser_path(
        &manifest,
        &package_root,
        requested_path,
        requested_specifier,
    ))
}

fn resolve_package_browser_subpath(
    manifest: &PackageManifest,
    package_root: &str,
    remainder: &str,
) -> BrowserMappingResolution {
    let requested = normalize_posix_path(&format!("{package_root}/{remainder}"));
    resolve_package_browser_path(manifest, package_root, &requested, remainder)
}

fn resolve_legacy_browser_entry(
    manifest: &PackageManifest,
    package_root: &str,
    entry: &str,
) -> BrowserMappingResolution {
    let candidate = normalize_posix_path(&format!("{package_root}/{entry}"));
    resolve_package_browser_path(manifest, package_root, &candidate, entry)
}

fn resolve_package_browser_path(
    manifest: &PackageManifest,
    package_root: &str,
    requested_path: &str,
    fallback: &str,
) -> BrowserMappingResolution {
    let Some(browser_field) = manifest.browser.as_ref().and_then(JsonValue::as_object) else {
        return BrowserMappingResolution::NotMapped;
    };
    let Some(browser_subpath) = to_package_browser_subpath(requested_path, package_root) else {
        return BrowserMappingResolution::NotMapped;
    };
    match resolve_browser_object_mapping(browser_field, &browser_subpath) {
        BrowserMappingResolution::Target(mapped) => {
            if mapped.starts_with('.') {
                BrowserMappingResolution::Target(normalize_posix_path(&format!(
                    "{package_root}/{mapped}"
                )))
            } else if mapped.starts_with('/') {
                BrowserMappingResolution::Target(normalize_posix_path(&mapped))
            } else {
                BrowserMappingResolution::Target(normalize_posix_path(&format!(
                    "{package_root}/{fallback}"
                )))
            }
        }
        other => other,
    }
}

fn resolve_browser_object_mapping(
    browser_field: &serde_json::Map<String, JsonValue>,
    subpath: &str,
) -> BrowserMappingResolution {
    for candidate in build_browser_subpath_candidates(subpath) {
        let Some(mapped) = browser_field.get(&candidate) else {
            continue;
        };

        return match mapped {
            JsonValue::String(value) if !value.is_empty() => {
                BrowserMappingResolution::Target(value.clone())
            }
            JsonValue::Bool(false) => BrowserMappingResolution::Blocked,
            _ => BrowserMappingResolution::NotMapped,
        };
    }

    BrowserMappingResolution::NotMapped
}

fn build_browser_subpath_candidates(subpath: &str) -> Vec<String> {
    let normalized = normalize_browser_subpath(subpath);
    if let Some(stripped) = normalized.strip_prefix("./") {
        vec![normalized.clone(), stripped.to_string()]
    } else {
        vec![normalized]
    }
}

fn normalize_browser_subpath(subpath: &str) -> String {
    if subpath == "." {
        ".".into()
    } else if subpath.starts_with("./") {
        subpath.to_string()
    } else {
        format!("./{}", subpath.trim_start_matches('/'))
    }
}

fn to_package_browser_subpath(requested_path: &str, package_root: &str) -> Option<String> {
    if requested_path == package_root {
        return Some(".".into());
    }
    if !requested_path.starts_with(&format!("{package_root}/")) {
        return None;
    }
    Some(format!(".{}", &requested_path[package_root.len()..]))
}

fn resolve_package_export_target(
    exports_field: &JsonValue,
    subpath: &str,
) -> PackageExportResolution {
    match exports_field {
        JsonValue::String(value) => {
            if subpath == "." {
                PackageExportResolution::Target(value.clone())
            } else {
                PackageExportResolution::Missing
            }
        }
        JsonValue::Null => {
            if subpath == "." {
                PackageExportResolution::Blocked
            } else {
                PackageExportResolution::Missing
            }
        }
        JsonValue::Object(map) => {
            if has_conditional_export_keys(map) {
                if subpath == "." {
                    resolve_conditional_export_value(exports_field)
                } else {
                    PackageExportResolution::Missing
                }
            } else {
                if let Some(value) = map.get(subpath) {
                    let resolved = resolve_conditional_export_value(value);
                    if !matches!(resolved, PackageExportResolution::Missing) {
                        return resolved;
                    }
                }

                if subpath == "." {
                    if let Some(value) = map.get(".") {
                        let resolved = resolve_conditional_export_value(value);
                        if !matches!(resolved, PackageExportResolution::Missing) {
                            return resolved;
                        }
                    }
                }

                resolve_wildcard_export_target(map, subpath)
            }
        }
        _ => PackageExportResolution::Missing,
    }
}

fn resolve_conditional_export_value(value: &JsonValue) -> PackageExportResolution {
    match value {
        JsonValue::String(target) => PackageExportResolution::Target(target.clone()),
        JsonValue::Null => PackageExportResolution::Blocked,
        JsonValue::Object(map) => {
            for condition in ["browser", "import", "module", "default", "require"] {
                if let Some(nested) = map.get(condition) {
                    let resolved = resolve_conditional_export_value(nested);
                    if !matches!(resolved, PackageExportResolution::Missing) {
                        return resolved;
                    }
                }
            }
            PackageExportResolution::Missing
        }
        _ => PackageExportResolution::Missing,
    }
}

fn resolve_wildcard_export_target(
    exports_field: &serde_json::Map<String, JsonValue>,
    subpath: &str,
) -> PackageExportResolution {
    for (key, value) in exports_field {
        if !key.contains('*') {
            continue;
        }

        let mut parts = key.splitn(2, '*');
        let prefix = parts.next().unwrap_or_default();
        let suffix = parts.next().unwrap_or_default();

        if !subpath.starts_with(prefix) || !subpath.ends_with(suffix) {
            continue;
        }

        let matched = &subpath[prefix.len()..subpath.len().saturating_sub(suffix.len())];
        match resolve_conditional_export_value(value) {
            PackageExportResolution::Target(target) => {
                let replaced = target.replace('*', matched);
                return PackageExportResolution::Target(replaced);
            }
            PackageExportResolution::Blocked => return PackageExportResolution::Blocked,
            PackageExportResolution::Missing => continue,
        }
    }

    PackageExportResolution::Missing
}

fn has_conditional_export_keys(map: &serde_json::Map<String, JsonValue>) -> bool {
    ["browser", "import", "module", "default", "require"]
        .iter()
        .any(|key| map.contains_key(*key))
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

fn resolve_nearest_package_root(record: &SessionRecord, importer_dir: &str) -> Option<String> {
    let mut current = normalize_posix_path(importer_dir);

    while current.starts_with("/workspace") {
        let package_json_path = format!("{current}/package.json");
        if record.vfs.read(&package_json_path).is_some() {
            return Some(current);
        }

        if current == "/workspace" {
            break;
        }

        current = dirname(&current).to_string();
    }

    None
}

fn read_package_manifest_at(record: &SessionRecord, package_root: &str) -> Option<PackageManifest> {
    let package_json_path = format!("{package_root}/package.json");
    record
        .vfs
        .read(&package_json_path)
        .and_then(|file| serde_json::from_slice::<PackageManifest>(&file.bytes).ok())
}

fn node_module_search_roots(importer_dir: &str, package_name: &str) -> Vec<String> {
    let mut roots = BTreeSet::new();
    let mut current = importer_dir.to_string();

    while current.starts_with("/workspace") {
        if current.ends_with("/node_modules") {
            roots.insert(normalize_posix_path(&format!("{current}/{package_name}")));
        } else {
            roots.insert(normalize_posix_path(&format!(
                "{current}/node_modules/{package_name}"
            )));
        }

        if current == "/workspace" {
            break;
        }

        current = dirname(&current).to_string();
    }

    roots.into_iter().collect()
}

fn node_module_directory_roots(importer_dir: &str) -> Vec<String> {
    let mut roots = BTreeSet::new();
    let mut current = importer_dir.to_string();

    while current.starts_with("/workspace") {
        if current.ends_with("/node_modules") {
            roots.insert(normalize_posix_path(&current));
        } else {
            roots.insert(normalize_posix_path(&format!("{current}/node_modules")));
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
