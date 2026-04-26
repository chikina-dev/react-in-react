use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::engine::{
    EngineAdapter, EngineBootstrapBridge, EngineBridgeSnapshot, EngineContextHandle,
    EngineContextSnapshot, EngineContextSpec, EngineContextState, EngineEvalMode,
    EngineEvalOutcome, EngineEvalRequest, EngineJobDrain, EngineSessionHandle, EngineSessionSpec,
};
use crate::error::{RuntimeHostError, RuntimeHostResult};
use crate::protocol::{
    ArchiveEntryKind, ArchiveEntrySummary, ArchiveStats, CapabilityMatrix, HostBootstrapSummary,
    HostContextFsCommand, HostFsCommand, HostFsResponse, HostProcessInfo, HostRuntimeBindings,
    HostRuntimeBootstrapModule, HostRuntimeBootstrapPlan, HostRuntimeBuiltinSpec,
    HostRuntimeCommand, HostRuntimeConsoleLevel, HostRuntimeContext, HostRuntimeEngineBoot,
    HostRuntimeEvent, HostRuntimeHttpRequest, HostRuntimeHttpServer, HostRuntimeHttpServerKind,
    HostRuntimeIdleReport, HostRuntimeLaunchReport, HostRuntimeLoadedModule,
    HostRuntimeModuleImportPlan, HostRuntimeModuleLoaderPlan, HostRuntimeModuleRecord,
    HostRuntimeModuleSource, HostRuntimePort, HostRuntimePortProtocol,
    HostRuntimePreviewLaunchReport, HostRuntimePreviewModel, HostRuntimePreviewRequestReport,
    HostRuntimePreviewImportPlan, HostRuntimePreviewModulePlan, HostRuntimePreviewReadyReport,
    HostRuntimePreviewStateReport, HostRuntimePreviewTransformKind,
    HostRuntimePreviewClientModule,
    HostRuntimeResolvedModule, HostRuntimeResponse, HostRuntimeShutdownReport,
    HostRuntimeStartupReport, HostRuntimeStateReport, HostRuntimeStdioStream, HostRuntimeTimer,
    HostRuntimeTimerKind, HostSessionStateReport, HostWorkspaceFileIndexSummary,
    PackageJsonSummary, PreviewRequestHint, PreviewRequestKind, PreviewResponseDescriptor,
    PreviewResponseKind, RunPlan, RunRequest, SessionSnapshot, SessionState, WorkspaceEntrySummary,
    WorkspaceFilePayload, WorkspaceFileSummary, HostRuntimeDirectHttpResponse,
};
use crate::vfs::{VirtualFile, VirtualFileSystem, normalize_posix_path};

const PREVIEW_DOCUMENT_CANDIDATES: [&str; 4] = [
    "/workspace/index.html",
    "/workspace/dist/index.html",
    "/workspace/build/index.html",
    "/workspace/public/index.html",
];

const PREVIEW_APP_ENTRY_CANDIDATES: [&str; 12] = [
    "/workspace/src/main.tsx",
    "/workspace/src/main.jsx",
    "/workspace/src/main.ts",
    "/workspace/src/main.js",
    "/workspace/src/index.tsx",
    "/workspace/src/index.jsx",
    "/workspace/src/index.ts",
    "/workspace/src/index.js",
    "/workspace/app/routes/home.tsx",
    "/workspace/app/routes/home.jsx",
    "/workspace/app/routes/index.tsx",
    "/workspace/app/routes/index.jsx",
];

const PREVIEW_GUEST_COMPONENT_CANDIDATES: [&str; 4] = [
    "/workspace/app/routes/home.tsx",
    "/workspace/app/routes/home.jsx",
    "/workspace/app/routes/index.tsx",
    "/workspace/app/routes/index.jsx",
];

const PREVIEW_GUEST_STYLESHEET_CANDIDATES: [&str; 4] = [
    "/workspace/app/app.css",
    "/workspace/src/index.css",
    "/workspace/src/App.css",
    "/workspace/src/app.css",
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewGuestAppShell {
    entry_url: String,
    stylesheet_urls: Vec<String>,
    react_url: String,
    react_dom_client_url: String,
}

fn find_preview_client_module_url<'a>(
    modules: &'a [HostRuntimePreviewClientModule],
    specifier: &str,
) -> Option<&'a str> {
    modules
        .iter()
        .find(|module| module.specifier == specifier)
        .map(|module| module.url.as_str())
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
    style: Option<String>,
    bin: Option<JsonValue>,
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
    run_plan: RunPlan,
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

fn split_command_tokens(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

struct BrowserCliShimSpec {
    package_name: &'static str,
    specifier: &'static str,
    mode: &'static str,
    runtime_kind: &'static str,
    preview_role: &'static str,
}

struct BuiltinBootstrapModuleSpec {
    specifier: &'static str,
    source: &'static str,
}

const BROWSER_CLI_SHIMS: &[BrowserCliShimSpec] = &[
    BrowserCliShimSpec {
        package_name: "vite",
        specifier: "runtime:browser-cli/vite",
        mode: "dev",
        runtime_kind: "browser-dev-server",
        preview_role: "http-server",
    },
    BrowserCliShimSpec {
        package_name: "acme-dev",
        specifier: "runtime:browser-cli/acme-dev",
        mode: "dev",
        runtime_kind: "browser-dev-server",
        preview_role: "http-server",
    },
];

const PREVIEW_AUXILIARY_ROUTES: &[(&str, PreviewRequestKind)] = &[
    ("/__runtime.json", PreviewRequestKind::RuntimeState),
    ("/__bootstrap.json", PreviewRequestKind::BootstrapState),
    ("/__workspace.json", PreviewRequestKind::WorkspaceState),
    ("/__files.json", PreviewRequestKind::FileIndex),
    ("/__diagnostics.json", PreviewRequestKind::DiagnosticsState),
    ("/assets/runtime.css", PreviewRequestKind::RuntimeStylesheet),
];

const BUILTIN_BOOTSTRAP_MODULE_SPECS: &[BuiltinBootstrapModuleSpec] = &[
    BuiltinBootstrapModuleSpec {
        specifier: "node:process",
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
"#,
    },
    BuiltinBootstrapModuleSpec {
        specifier: "node:fs",
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
"#,
    },
    BuiltinBootstrapModuleSpec {
        specifier: "node:path",
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
"#,
    },
    BuiltinBootstrapModuleSpec {
        specifier: "node:buffer",
        source: r#"export const Buffer = Uint8Array;
export default { Buffer };
"#,
    },
    BuiltinBootstrapModuleSpec {
        specifier: "node:timers",
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
"#,
    },
    BuiltinBootstrapModuleSpec {
        specifier: "node:console",
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
"#,
    },
    BuiltinBootstrapModuleSpec {
        specifier: "node:perf_hooks",
        source: r#"const performanceValue =
  globalThis.performance ??
  {
    now() {
      return Date.now();
    },
  };
export const performance = performanceValue;
export default { performance };
"#,
    },
    BuiltinBootstrapModuleSpec {
        specifier: "node:module",
        source: r#"const moduleValue = {
  enableCompileCache() {},
  flushCompileCache() {},
};
export const enableCompileCache = (...args) =>
  moduleValue.enableCompileCache(...args);
export const flushCompileCache = (...args) =>
  moduleValue.flushCompileCache(...args);
export default moduleValue;
"#,
    },
    BuiltinBootstrapModuleSpec {
        specifier: "node:inspector",
        source: r#"class Session {
  connect() {}
  post(_method, callback) {
    if (typeof callback === "function") {
      callback();
    }
  }
  disconnect() {}
}
const inspectorValue = { Session };
export { Session };
export default inspectorValue;
"#,
    },
];

fn render_browser_cli_shim_source(shim: &BrowserCliShimSpec) -> String {
    format!(
        r#"const runtimeValue = {{
  mode: {mode},
  runtimeKind: {runtime_kind},
  previewRole: {preview_role},
  ready: true,
}};
globalThis.__browserCliRuntime = runtimeValue;
export default runtimeValue;
"#,
        mode = serde_json::to_string(shim.mode)
            .expect("browser cli shim mode should serialize as json string"),
        runtime_kind = serde_json::to_string(shim.runtime_kind)
            .expect("browser cli shim runtime kind should serialize as json string"),
        preview_role = serde_json::to_string(shim.preview_role)
            .expect("browser cli shim preview role should serialize as json string"),
    )
}

fn manifest_declares_bin_command(manifest: &PackageManifest, command_name: &str) -> bool {
    match manifest.bin.as_ref() {
        Some(JsonValue::String(_)) => manifest
            .name
            .as_deref()
            .is_some_and(|name| name == command_name),
        Some(JsonValue::Object(map)) => map.contains_key(command_name),
        _ => false,
    }
}

fn find_browser_cli_shim_for_manifest(
    manifest: &PackageManifest,
    command_name: &str,
    args: &[String],
) -> Option<&'static BrowserCliShimSpec> {
    if !args.is_empty() {
        return None;
    }

    let package_name = manifest.name.as_deref()?;
    if !manifest_declares_bin_command(manifest, command_name) {
        return None;
    }

    BROWSER_CLI_SHIMS
        .iter()
        .find(|shim| shim.package_name == package_name)
}

fn find_browser_cli_shim_by_specifier(specifier: &str) -> Option<&'static BrowserCliShimSpec> {
    BROWSER_CLI_SHIMS
        .iter()
        .find(|shim| shim.specifier == specifier)
}

fn find_browser_cli_shim_for_resolved_script(
    resolved_script: &str,
) -> Option<&'static BrowserCliShimSpec> {
    let command_name = split_command_tokens(resolved_script).into_iter().next()?;
    BROWSER_CLI_SHIMS
        .iter()
        .find(|shim| shim.package_name == command_name)
}

fn find_preview_auxiliary_route(relative_path: &str) -> Option<PreviewRequestKind> {
    PREVIEW_AUXILIARY_ROUTES
        .iter()
        .find(|(path, _)| *path == relative_path)
        .map(|(_, kind)| kind.clone())
}

fn resolve_npm_script_process_entrypoint(
    record: &SessionRecord,
    cwd: &str,
    plan: &RunPlan,
) -> RuntimeHostResult<(String, Vec<String>)> {
    let tokens = plan
        .resolved_script
        .as_deref()
        .map(split_command_tokens)
        .unwrap_or_default();

    if let Some((entrypoint, args)) = tokens.split_first() {
        if entrypoint.starts_with("./") || entrypoint.starts_with("../") || entrypoint.starts_with('/')
        {
            return Ok((
                resolve_node_entrypoint(record, cwd, Some(entrypoint))?,
                args.to_vec(),
            ));
        }

        for root in node_module_directory_roots(cwd) {
            let package_root = normalize_posix_path(&format!("{root}/{entrypoint}"));
            if let Some(manifest) = read_package_manifest_at(record, &package_root) {
                if let Some(shim) =
                    find_browser_cli_shim_for_manifest(&manifest, entrypoint, args)
                {
                    return Ok((String::from(shim.specifier), Vec::new()));
                }

                if let Some(resolved) = resolve_package_bin_entrypoint(
                    record,
                    &package_root,
                    entrypoint,
                    &manifest,
                )? {
                    return Ok((resolved, args.to_vec()));
                }
            }

            let candidate = normalize_posix_path(&format!("{root}/.bin/{entrypoint}"));
            if record.vfs.read(&candidate).is_some() {
                return Ok((candidate, args.to_vec()));
            }
        }

        return Err(RuntimeHostError::ModuleNotFound(entrypoint.clone()));
    }

    Ok((plan.entrypoint.clone(), Vec::new()))
}

fn resolve_package_bin_entrypoint(
    record: &SessionRecord,
    package_root: &str,
    command_name: &str,
    manifest: &PackageManifest,
) -> RuntimeHostResult<Option<String>> {
    match manifest.bin.as_ref() {
        Some(JsonValue::String(target)) => {
            resolve_workspace_module(record, &normalize_posix_path(&format!("{package_root}/{target}")))
                .map(Some)
        }
        Some(JsonValue::Object(map)) => {
            let Some(target) = map.get(command_name).and_then(JsonValue::as_str) else {
                return Ok(None);
            };
            resolve_workspace_module(record, &normalize_posix_path(&format!("{package_root}/{target}")))
                .map(Some)
        }
        _ => Ok(None),
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
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let (entrypoint, argv) = match plan.command_kind {
            crate::protocol::RunCommandKind::NodeEntrypoint => {
                let entrypoint = plan.entrypoint.clone();
                let mut argv = vec![String::from("/virtual/node"), entrypoint.clone()];
                argv.extend(request.args.iter().skip(1).cloned());
                (entrypoint, argv)
            }
            crate::protocol::RunCommandKind::NpmScript => {
                let (entrypoint, script_args) =
                    resolve_npm_script_process_entrypoint(record, &plan.cwd, &plan)?;
                let mut argv = vec![String::from("/virtual/node"), entrypoint.clone()];
                argv.extend(script_args);
                argv.extend(request.args.iter().skip(2).cloned());
                (entrypoint, argv)
            }
        };

        Ok(HostProcessInfo {
            cwd: plan.cwd,
            argv,
            env: request.env.clone(),
            exec_path: String::from("/virtual/node"),
            platform: String::from("browser"),
            entrypoint,
            command_line: plan.command_line,
            command_kind: plan.command_kind,
        })
    }

    pub fn create_runtime_context(
        &mut self,
        session_id: &str,
        request: &RunRequest,
    ) -> RuntimeHostResult<HostRuntimeContext> {
        let run_plan = self.plan_run(session_id, request)?;
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
                run_plan,
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

    fn drain_runtime_events(
        &mut self,
        context_id: &str,
    ) -> RuntimeHostResult<Vec<HostRuntimeEvent>> {
        let context = self
            .runtime_contexts
            .get_mut(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        Ok(context.events.drain(..).collect())
    }

    fn boot_runtime_engine(
        &mut self,
        context_id: &str,
    ) -> RuntimeHostResult<HostRuntimeEngineBoot> {
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
        let import_plans =
            self.collect_runtime_boot_import_graph(context_id, &plan, &loader_plan)?;
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
                root_report: None,
            });
        }

        let server = match self
            .execute_runtime_command(context_id, HostRuntimeCommand::HttpServePreview { port })?
        {
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
            client_modules: Vec::new(),
        };

        let root_report = self.resolve_runtime_preview_request(context_id, request.clone())?;

        Ok(HostRuntimePreviewLaunchReport {
            startup,
            root_report: Some(Box::new(root_report)),
        })
    }

    pub fn launch_runtime(
        &mut self,
        session_id: &str,
        request: &RunRequest,
        max_turns: usize,
        port: Option<u16>,
    ) -> RuntimeHostResult<HostRuntimeLaunchReport> {
        if let Some(record) = self.sessions.get_mut(session_id) {
            record.snapshot.state = SessionState::Running;
        }
        let boot_summary = self.boot_summary();
        let run_plan = self.plan_run(session_id, request)?;
        let capabilities = self
            .sessions
            .get(session_id)
            .map(|record| record.snapshot.capabilities.clone())
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.to_string()))?;
        let runtime_context = self.create_runtime_context(session_id, request)?;
        let context_id = runtime_context.context_id.clone();
        let runtime_context_record = self
            .runtime_contexts
            .get(&context_id)
            .cloned()
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.clone()))?;
        let bindings = build_runtime_bindings(
            &context_id,
            &runtime_context_record,
            self.engine.descriptor(),
        );
        let bootstrap_plan = build_runtime_bootstrap_plan(&bindings);
        let preview_launch = self.launch_runtime_preview(&context_id, max_turns, port)?;
        let runtime_context_record = self
            .runtime_contexts
            .get(&context_id)
            .cloned()
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.clone()))?;
        let state = self.runtime_state_report(&context_id)?;
        let engine_context = self.describe_engine_context(&context_id)?;
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
        let preview_ready = state.preview.as_ref().map(Self::preview_ready_report);
        let startup_stdout = build_runtime_startup_stdout(
            &state.session,
            &run_plan,
            &startup_logs,
            preview_launch.startup.exit_code,
            preview_launch.startup.exited,
            preview_ready.as_ref(),
        );
        let events = self.drain_runtime_events(&context_id)?;
        let runtime_context = HostRuntimeContext {
            context_id: context_id.clone(),
            session_id: runtime_context_record.session_id.clone(),
            process: runtime_context_record.process.clone(),
        };

        Ok(HostRuntimeLaunchReport {
            boot_summary,
            run_plan,
            runtime_context,
            engine_context,
            bindings,
            bootstrap_plan,
            preview_launch,
            state,
            startup_stdout,
            preview_ready,
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
        self.ensure_runtime_preview_request_ready(context_id)?;
        let request_hint =
            self.resolve_preview_request_hint(&session_id, &request.relative_path)?;
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
                .collect::<Vec<_>>()
        };
        let module_plan = build_runtime_preview_module_plan(
            self,
            context_id,
            request.port,
            &request_hint,
            &response_descriptor,
        )?;
        let transform_kind = determine_runtime_preview_transform_kind(
            &request_hint,
            &response_descriptor,
            module_plan.as_ref(),
        );
        let direct_response = self.build_runtime_direct_preview_response(
            context_id,
            &session_id,
            &request,
            &request_hint,
            &response_descriptor,
            transform_kind.as_ref(),
            &hydrated_files,
        )?;
        let render_plan = build_runtime_preview_render_plan(
            &request_hint,
            &response_descriptor,
        );

        Ok(HostRuntimePreviewRequestReport {
            server: runtime_http_server_view(&server),
            port: runtime_port_view(&server.port),
            request,
            request_hint,
            response_descriptor,
            hydration_paths,
            hydrated_files,
            transform_kind,
            render_plan,
            module_plan,
            direct_response,
        })
    }

    fn ensure_runtime_preview_request_ready(&self, context_id: &str) -> RuntimeHostResult<()> {
        let descriptor = self.engine.descriptor();
        if descriptor.name != "quickjs-ng-browser-vm-harness"
            && descriptor.name != "quickjs-ng-browser-c-vm"
        {
            return Ok(());
        }

        let snapshot = self.describe_engine_context(context_id)?;
        let bootstrap_ready = snapshot.state == EngineContextState::Ready
            && snapshot.bootstrap_specifier.as_deref() == Some("runtime:bootstrap")
            && snapshot.bridge_ready
            && snapshot.registered_modules > 0;
        if bootstrap_ready {
            return Ok(());
        }

        Err(RuntimeHostError::EngineFailure(format!(
            "browser preview request requires bootstrap-ready engine context: state={:?} bootstrap={:?} bridge_ready={} modules={}",
            snapshot.state,
            snapshot.bootstrap_specifier,
            snapshot.bridge_ready,
            snapshot.registered_modules
        )))
    }

    fn build_runtime_direct_preview_response(
        &self,
        context_id: &str,
        session_id: &str,
        request: &HostRuntimeHttpRequest,
        request_hint: &PreviewRequestHint,
        response_descriptor: &PreviewResponseDescriptor,
        transform_kind: Option<&HostRuntimePreviewTransformKind>,
        hydrated_files: &[WorkspaceFilePayload],
    ) -> RuntimeHostResult<Option<HostRuntimeDirectHttpResponse>> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let session_state = self.session_state_report(session_id)?;
        let preview_model = Self::preview_model(session_id, &context.run_plan, &session_state);
        let preview_url = preview_request_url(session_id, request.port);

        let response = match response_descriptor.kind {
            PreviewResponseKind::WorkspaceDocument => response_descriptor
                .workspace_path
                .as_ref()
                .and_then(|path| record.vfs.read(path))
                .map(|file| {
                    build_preview_direct_workspace_file_response(
                        record,
                        session_id,
                        request.port,
                        file,
                        response_descriptor,
                        transform_kind,
                        Some(&preview_url),
                    )
                }),
            PreviewResponseKind::AppShell => response_descriptor.workspace_path.as_ref().and_then(|path| {
                if is_preview_guest_component_path(path) {
                    resolve_preview_guest_app_shell_for_path(
                        self,
                        context_id,
                        record,
                        session_id,
                        request.port,
                        path,
                        &request.client_modules,
                    )
                    .map(|guest_app_shell| HostRuntimeDirectHttpResponse {
                        status: 200,
                        headers: BTreeMap::new(),
                        text_body: Some(render_preview_guest_app_shell_html(
                            &preview_model.title,
                            &guest_app_shell,
                        )),
                        bytes_body: None,
                    })
                    .or_else(|| Some(render_preview_root_html(
                        session_id,
                        request.port,
                        &preview_model,
                        &request.client_modules,
                    )))
                } else {
                    let entry_url = build_preview_url_for_workspace_file(
                        session_id,
                        request.port,
                        path,
                        response_descriptor
                            .document_root
                            .as_deref()
                            .unwrap_or("/workspace"),
                    );
                    Some(HostRuntimeDirectHttpResponse {
                        status: 200,
                        headers: BTreeMap::new(),
                        text_body: Some(render_preview_app_shell_html(&preview_model.title, &entry_url)),
                        bytes_body: None,
                    })
                }
            }),
            PreviewResponseKind::HostManagedFallback => Some(render_preview_root_html(
                session_id,
                request.port,
                &preview_model,
                &request.client_modules,
            )),
            PreviewResponseKind::WorkspaceFile => response_descriptor
                .workspace_path
                .as_ref()
                .and_then(|path| {
                    transform_kind
                        .filter(|kind| !matches!(kind, HostRuntimePreviewTransformKind::Module))
                        .and_then(|kind| {
                            record.vfs.read(path).map(|file| {
                                build_preview_direct_workspace_file_response(
                                    record,
                                    session_id,
                                    request.port,
                                    file,
                                    response_descriptor,
                                    Some(kind),
                                    Some(&preview_url),
                                )
                            })
                        })
                }),
            PreviewResponseKind::WorkspaceAsset => response_descriptor
                .workspace_path
                .as_ref()
                .and_then(|path| {
                    transform_kind
                        .filter(|kind| !matches!(kind, HostRuntimePreviewTransformKind::Module))
                        .and_then(|kind| {
                            record.vfs.read(path).map(|file| {
                                build_preview_direct_workspace_file_response(
                                    record,
                                    session_id,
                                    request.port,
                                    file,
                                    response_descriptor,
                                    Some(kind),
                                    Some(&preview_url),
                                )
                            })
                        })
                }),
            PreviewResponseKind::MethodNotAllowed => Some(HostRuntimeDirectHttpResponse {
                status: 405,
                headers: BTreeMap::new(),
                text_body: Some(
                    serde_json::json!({
                        "error": "Method not allowed",
                        "pathname": request.relative_path,
                        "method": request.method,
                        "allowMethods": response_descriptor.allow_methods,
                    })
                    .to_string(),
                ),
                bytes_body: None,
            }),
            PreviewResponseKind::BootstrapState => self
                .preview_state_report(context_id)?
                .as_ref()
                .map(|preview| {
                    render_preview_json_response(render_preview_bootstrap_json(
                        record,
                        &session_state,
                        preview,
                        request_hint,
                        hydrated_files,
                    ))
                }),
            PreviewResponseKind::RuntimeState => self
                .preview_state_report(context_id)?
                .as_ref()
                .map(|preview| render_preview_json_response(render_preview_ready_event_json(preview))),
            PreviewResponseKind::WorkspaceState => {
                Some(render_preview_json_response(render_session_snapshot_json(
                    &session_state,
                )))
            }
            PreviewResponseKind::FileIndex => self
                .preview_state_report(context_id)?
                .as_ref()
                .map(|preview| {
                    render_preview_json_response(render_preview_file_index_json(
                        session_id,
                        preview,
                        &session_state.host_files.index,
                    ))
                }),
            PreviewResponseKind::DiagnosticsState => self
                .preview_state_report(context_id)?
                .as_ref()
                .map(|preview| {
                    render_preview_json_response(render_preview_diagnostics_json(
                        &session_state,
                        preview,
                        request_hint,
                        hydrated_files,
                    ))
                }),
            PreviewResponseKind::RuntimeStylesheet => Some(HostRuntimeDirectHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                text_body: Some(render_runtime_stylesheet()),
                bytes_body: None,
            }),
            PreviewResponseKind::NotFound => Some(HostRuntimeDirectHttpResponse {
                status: 404,
                headers: BTreeMap::new(),
                text_body: Some(
                    serde_json::json!({
                        "error": "Unsupported preview path",
                        "pathname": request.relative_path,
                    })
                    .to_string(),
                ),
                bytes_body: None,
            }),
        };

        Ok(response.map(|response| apply_direct_preview_response_metadata(response, response_descriptor)))
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
        if let Some(session) = self.sessions.get_mut(&context.session_id) {
            session.snapshot.state = SessionState::Stopped;
        }

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
        let entry_module = if let Some(module) = plan
            .modules
            .iter()
            .find(|module| module.specifier == loader_plan.entry_module.resolved_specifier)
        {
            HostRuntimeLoadedModule {
                resolved_specifier: module.specifier.clone(),
                kind: crate::protocol::HostRuntimeModuleKind::Registered,
                format: crate::protocol::HostRuntimeModuleFormat::Module,
                source: module.source.clone(),
            }
        } else {
            self.load_runtime_module(context_id, &loader_plan.entry_module.resolved_specifier)?
        };
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

    fn workspace_file_index_summary(
        &self,
        session_id: &str,
    ) -> RuntimeHostResult<HostWorkspaceFileIndexSummary> {
        let index = self.workspace_file_summaries(session_id)?;
        let sample = index.first().cloned();

        Ok(HostWorkspaceFileIndexSummary {
            count: index.len(),
            index,
            sample_path: sample.as_ref().map(|file| file.path.clone()),
            sample_size: sample.as_ref().map(|file| file.size),
        })
    }

    fn session_archive_entries(record: &SessionRecord) -> Vec<ArchiveEntrySummary> {
        let workspace_root = record.snapshot.workspace_root.as_str();
        let mut entries = record
            .vfs
            .directories()
            .filter(|path| path.as_str() != workspace_root)
            .map(|path| ArchiveEntrySummary {
                path: path.clone(),
                size: 0,
                kind: ArchiveEntryKind::Directory,
            })
            .collect::<Vec<_>>();

        entries.extend(record.vfs.files().map(|file| ArchiveEntrySummary {
            path: file.path.clone(),
            size: file.bytes.len(),
            kind: ArchiveEntryKind::File,
        }));
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        entries
    }

    fn session_package_json_summary(record: &SessionRecord) -> Option<PackageJsonSummary> {
        read_package_manifest(&record.vfs).map(|manifest| PackageJsonSummary {
            name: manifest.name,
            scripts: manifest.scripts.unwrap_or_default(),
            dependencies: manifest
                .dependencies
                .map(|deps| deps.into_keys().collect())
                .unwrap_or_default(),
            dev_dependencies: manifest
                .dev_dependencies
                .map(|deps| deps.into_keys().collect())
                .unwrap_or_default(),
        })
    }

    fn preview_model(
        session_id: &str,
        run_plan: &RunPlan,
        session: &HostSessionStateReport,
    ) -> HostRuntimePreviewModel {
        let package_name = session
            .package_json
            .as_ref()
            .and_then(|summary| summary.name.clone())
            .unwrap_or_else(|| session.archive.file_name.clone());

        HostRuntimePreviewModel {
            title: format!("{package_name} guest app"),
            summary: String::from(
                "Host React から iframe 内 DOM に別 root を生やして描画しています。次の段階でこの生成責務を Service Worker + WASM host へ寄せます。",
            ),
            cwd: run_plan.cwd.clone(),
            command: run_plan.command_line.clone(),
            highlights: vec![
                format!("session={session_id}"),
                format!("revision={}", session.revision),
                format!("files={}", session.archive.file_count),
                format!(
                    "run-kind={}",
                    match run_plan.command_kind {
                        crate::protocol::RunCommandKind::NpmScript => "npm-script",
                        crate::protocol::RunCommandKind::NodeEntrypoint => "node-entrypoint",
                    }
                ),
                run_plan
                    .resolved_script
                    .as_ref()
                    .map(|script| format!("resolved-script={script}"))
                    .unwrap_or_else(|| String::from("resolved-script=<direct>")),
                format!("react-detected={}", session.capabilities.detected_react),
            ],
        }
    }

    fn preview_ready_report(preview: &HostRuntimePreviewStateReport) -> HostRuntimePreviewReadyReport {
        HostRuntimePreviewReadyReport {
            port: preview.port.clone(),
            url: preview.url.clone(),
            model: preview.model.clone(),
            root_hydrated_files: preview.root_hydrated_files.clone(),
            host: preview.host.clone(),
            run: preview.run.clone(),
            host_files: preview.host_files.clone(),
        }
    }

    fn session_state_report(&self, session_id: &str) -> RuntimeHostResult<HostSessionStateReport> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;
        let package_json = Self::session_package_json_summary(record);

        Ok(HostSessionStateReport {
            session_id: record.snapshot.session_id.clone(),
            state: record.snapshot.state.clone(),
            revision: record.snapshot.revision,
            workspace_root: record.snapshot.workspace_root.clone(),
            archive: record.snapshot.archive.clone(),
            archive_entries: Self::session_archive_entries(record),
            suggested_run_request: suggested_run_request(
                package_json.as_ref(),
                &record.snapshot.workspace_root,
            ),
            package_json,
            capabilities: record.snapshot.capabilities.clone(),
            host_files: self.workspace_file_index_summary(session_id)?,
        })
    }

    fn preview_state_report(
        &self,
        context_id: &str,
    ) -> RuntimeHostResult<Option<HostRuntimePreviewStateReport>> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        let Some(server) = context.http_servers.values().next().cloned() else {
            return Ok(None);
        };
        let request = HostRuntimeHttpRequest {
            port: server.port.port,
            method: String::from("GET"),
            relative_path: String::from("/"),
            search: String::new(),
            client_modules: Vec::new(),
        };
        let report = self.resolve_runtime_preview_request(context_id, request.clone())?;

        let session_state = self.session_state_report(&context.session_id)?;

        let port = report.port.clone();
        Ok(Some(HostRuntimePreviewStateReport {
            port,
            url: format!("/preview/{}/{}/", context.session_id, report.port.port),
            model: Self::preview_model(&context.session_id, &context.run_plan, &session_state),
            root_request: request,
            root_request_hint: report.request_hint,
            root_response_descriptor: report.response_descriptor,
            root_hydrated_files: report.hydrated_files,
            host: self.boot_summary(),
            run: context.run_plan.clone(),
            host_files: self.workspace_file_index_summary(&context.session_id)?,
        }))
    }

    fn runtime_state_report(&self, context_id: &str) -> RuntimeHostResult<HostRuntimeStateReport> {
        let context = self
            .runtime_contexts
            .get(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;

        Ok(HostRuntimeStateReport {
            session: self.session_state_report(&context.session_id)?,
            preview: self.preview_state_report(context_id)?,
        })
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
            HostRuntimeCommand::DescribeState => Ok(HostRuntimeResponse::StateReport(
                self.runtime_state_report(context_id)?,
            )),
            HostRuntimeCommand::DescribeModuleLoader => Ok(HostRuntimeResponse::ModuleLoaderPlan(
                self.describe_runtime_module_loader(context_id)?,
            )),
            HostRuntimeCommand::BootEngine => Ok(HostRuntimeResponse::EngineBoot(
                self.boot_runtime_engine(context_id)?,
            )),
            HostRuntimeCommand::Startup { max_turns } => Ok(HostRuntimeResponse::StartupReport(
                self.run_runtime_startup(context_id, max_turns)?,
            )),
            HostRuntimeCommand::LaunchPreview { max_turns, port } => {
                Ok(HostRuntimeResponse::PreviewLaunchReport(
                    self.launch_runtime_preview(context_id, max_turns, port)?,
                ))
            }
            HostRuntimeCommand::PreviewRequest { request } => {
                Ok(HostRuntimeResponse::PreviewRequestReport(
                    self.resolve_runtime_preview_request(context_id, request)?,
                ))
            }
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
                        self.push_workspace_change_event(context_id, entry.clone(), revision)?;
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

        {
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
        }
        if let Some(revision) = revision {
            let state = self.runtime_state_report(context_id)?;
            let context = self
                .runtime_contexts
                .get_mut(context_id)
                .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
            for entry in changed_entries {
                context.events.push_back(HostRuntimeEvent::WorkspaceChange {
                    entry,
                    revision,
                    state: state.clone(),
                });
            }
        }

        Ok(())
    }

    fn push_workspace_change_event(
        &mut self,
        context_id: &str,
        entry: WorkspaceEntrySummary,
        revision: u64,
    ) -> RuntimeHostResult<()> {
        let state = self.runtime_state_report(context_id)?;
        let context = self
            .runtime_contexts
            .get_mut(context_id)
            .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?;
        context.events.push_back(HostRuntimeEvent::WorkspaceChange {
            entry,
            revision,
            state,
        });
        Ok(())
    }

    fn resolve_preview_root_hint(&self, session_id: &str) -> RuntimeHostResult<PreviewRootHint> {
        let record = self
            .sessions
            .get(session_id)
            .ok_or_else(|| RuntimeHostError::SessionNotFound(session_id.into()))?;

        let source_entry_candidate = PREVIEW_APP_ENTRY_CANDIDATES
            .iter()
            .find(|candidate| record.vfs.read(candidate).is_some())
            .copied();

        for candidate in PREVIEW_DOCUMENT_CANDIDATES {
            if let Some(file) = record.vfs.read(candidate) {
                if file.is_text && file.path.ends_with(".html") {
                    if candidate == "/workspace/public/index.html" {
                        if let Some(source_entry) = source_entry_candidate {
                            return Ok(PreviewRootHint {
                                kind: PreviewRootKind::SourceEntry,
                                path: Some(source_entry.to_string()),
                                root: None,
                            });
                        }
                    }
                    return Ok(PreviewRootHint {
                        kind: PreviewRootKind::WorkspaceDocument,
                        path: Some(file.path.clone()),
                        root: Some(dirname(candidate).to_string()),
                    });
                }
            }
        }

        if let Some(source_entry) = source_entry_candidate {
            return Ok(PreviewRootHint {
                kind: PreviewRootKind::SourceEntry,
                path: Some(source_entry.to_string()),
                root: None,
            });
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
            path if find_preview_auxiliary_route(path).is_some() => Ok(PreviewRequestHint {
                kind: find_preview_auxiliary_route(path).expect("route checked above"),
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

fn build_runtime_preview_module_plan<E: EngineAdapter>(
    host: &RuntimeHostCore<E>,
    context_id: &str,
    port: u16,
    request_hint: &PreviewRequestHint,
    response_descriptor: &PreviewResponseDescriptor,
) -> RuntimeHostResult<Option<HostRuntimePreviewModulePlan>> {
    let Some(workspace_path) =
        resolve_preview_module_plan_workspace_path(request_hint, response_descriptor)
    else {
        return Ok(None);
    };
    if !is_preview_module_workspace_path(workspace_path) {
        return Ok(None);
    }
    let loaded_module = match host.load_runtime_module(context_id, workspace_path) {
        Ok(module) => module,
        Err(_) => return Ok(None),
    };
    let session_id = host
        .runtime_contexts
        .get(context_id)
        .ok_or_else(|| RuntimeHostError::RuntimeContextNotFound(context_id.into()))?
        .session_id
        .clone();
    let document_root = response_descriptor
        .document_root
        .as_deref()
        .or(request_hint.document_root.as_deref())
        .unwrap_or("/workspace");
    let mut import_plans = Vec::new();

    for specifier in collect_module_dependency_specifiers(&loaded_module.source, &loaded_module.format) {
        if let Ok(plan) = host.prepare_runtime_module_import(context_id, &specifier, Some(workspace_path)) {
            let preview_specifier = render_preview_module_specifier(
                &session_id,
                port,
                &plan.resolved_module.resolved_specifier,
                document_root,
            );
            import_plans.push(HostRuntimePreviewImportPlan {
                request_specifier: specifier,
                preview_specifier,
                format: plan.loaded_module.format,
            });
        }
    }

    Ok(Some(HostRuntimePreviewModulePlan {
        importer_path: workspace_path.to_string(),
        format: loaded_module.format,
        import_plans,
    }))
}

fn is_preview_module_workspace_path(path: &str) -> bool {
    let normalized = path.to_ascii_lowercase();
    normalized.ends_with(".js")
        || normalized.ends_with(".mjs")
        || normalized.ends_with(".cjs")
        || normalized.ends_with(".ts")
        || normalized.ends_with(".tsx")
        || normalized.ends_with(".jsx")
        || normalized.ends_with(".mts")
        || normalized.ends_with(".cts")
        || normalized.ends_with(".json")
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
        boot_summary.engine_name,
        boot_summary.supports_interrupts,
        boot_summary.supports_module_loader
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
        "[engine-context] state={} pending-jobs={} bridge-ready={} entry={}",
        match engine_context.state {
            crate::engine::EngineContextState::Booted => "booted",
            crate::engine::EngineContextState::Ready => "ready",
            crate::engine::EngineContextState::Disposed => "disposed",
        },
        engine_context.pending_jobs,
        engine_context.bridge_ready,
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
    if let Some(shim) = find_browser_cli_shim_by_specifier(&run_plan.entrypoint).or_else(|| {
        run_plan
            .resolved_script
            .as_deref()
            .and_then(find_browser_cli_shim_for_resolved_script)
    }) {
        logs.push(format!(
            "[browser-cli] runtime={} preview={} mode={}",
            shim.runtime_kind, shim.preview_role, shim.mode
        ));
    }
    logs.push(format!("[context] id={}", context_id));
    logs.push(format!("[detect] react={}", capabilities.detected_react));

    logs
}

fn build_runtime_startup_stdout(
    session: &HostSessionStateReport,
    run_plan: &RunPlan,
    startup_logs: &[String],
    exit_code: Option<i32>,
    exited: bool,
    preview_ready: Option<&HostRuntimePreviewReadyReport>,
) -> Vec<String> {
    let mut lines = vec![format!(
        "[mount] {} files available at {}",
        session.archive.file_count, session.workspace_root
    )];

    lines.push(format!("[exec] {}", run_plan.command_line));

    if let Some(script) = &run_plan.resolved_script {
        lines.push(format!("[script] {script}"));
    }

    lines.push(format!(
        "[host-vfs] files={} sample={} size={}",
        session.host_files.count,
        session.host_files.sample_path.as_deref().unwrap_or("<none>"),
        session.host_files.sample_size.unwrap_or(0),
    ));
    lines.extend(startup_logs.iter().cloned());

    if exited {
        lines.push(format!(
            "[process] exited before preview code={}",
            exit_code.unwrap_or(0)
        ));
    } else if let Some(preview_ready) = preview_ready {
        lines.push(format!("[preview] server-ready {}", preview_ready.url));
    }

    lines
}

fn preview_request_url(session_id: &str, port: u16) -> String {
    format!("/preview/{session_id}/{port}/")
}

fn build_preview_url_for_workspace_file(
    session_id: &str,
    port: u16,
    workspace_path: &str,
    document_root: &str,
) -> String {
    let effective_root = if workspace_path.starts_with(&format!("{document_root}/")) {
        document_root
    } else {
        "/workspace"
    };
    let relative = workspace_path
        .strip_prefix(effective_root)
        .unwrap_or(workspace_path)
        .trim_start_matches('/');
    let preview_url = preview_request_url(session_id, port);
    if relative.is_empty() {
        preview_url
    } else {
        format!("{preview_url}{relative}")
    }
}

fn render_preview_module_specifier(
    session_id: &str,
    port: u16,
    resolved_specifier: &str,
    document_root: &str,
) -> String {
    if resolved_specifier.starts_with("/workspace") {
        build_preview_url_for_workspace_file(session_id, port, resolved_specifier, document_root)
    } else {
        resolved_specifier.to_string()
    }
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn rewrite_html_document(source: &str, preview_url: &str) -> String {
    rewrite_prefixed_attributes(source, preview_url, &["src", "href", "action", "poster"])
}

fn rewrite_stylesheet_document(
    record: &SessionRecord,
    source: &str,
    workspace_path: &str,
    session_id: &str,
    port: u16,
    preview_url: &str,
) -> String {
    let with_imports =
        rewrite_stylesheet_imports(record, source, workspace_path, session_id, port);
    let mut rewritten = String::with_capacity(with_imports.len());
    let mut index = 0usize;

    while index < with_imports.len() {
        if with_imports[index..].starts_with("url(") {
            rewritten.push_str("url(");
            index += 4;
            let Some(next_char) = with_imports[index..].chars().next() else {
                break;
            };
            if next_char == '"' || next_char == '\'' {
                let quote = next_char;
                rewritten.push(quote);
                index += quote.len_utf8();
                if with_imports[index..].starts_with('/') && !with_imports[index..].starts_with("//")
                {
                    rewritten.push_str(preview_url);
                    index += 1;
                    continue;
                }
                continue;
            }
            if with_imports[index..].starts_with('/') && !with_imports[index..].starts_with("//") {
                rewritten.push_str(preview_url);
                index += 1;
                continue;
            }
            continue;
        }
        let ch = with_imports[index..].chars().next().unwrap_or_default();
        rewritten.push(ch);
        index += ch.len_utf8();
    }

    rewritten
}

fn rewrite_stylesheet_imports(
    record: &SessionRecord,
    source: &str,
    workspace_path: &str,
    session_id: &str,
    port: u16,
) -> String {
    let mut rewritten = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(specifier) = parse_stylesheet_import_specifier(trimmed) {
            if let Some(resolved) =
                resolve_stylesheet_specifier(record, workspace_path, specifier.as_str())
            {
                let preview_specifier =
                    build_preview_url_for_workspace_file(session_id, port, &resolved, "/workspace");
                rewritten.push_str(&line.replacen(
                    specifier.as_str(),
                    preview_specifier.as_str(),
                    1,
                ));
                rewritten.push('\n');
                continue;
            }
        }
        rewritten.push_str(line);
        rewritten.push('\n');
    }
    if source.ends_with('\n') {
        rewritten
    } else {
        rewritten.trim_end_matches('\n').to_string()
    }
}

fn parse_stylesheet_import_specifier(line: &str) -> Option<String> {
    if !line.starts_with("@import") {
        return None;
    }

    let quoted = if let Some(start) = line.find('"') {
        let rest = &line[start + 1..];
        rest.find('"').map(|end| rest[..end].to_string())
    } else if let Some(start) = line.find('\'') {
        let rest = &line[start + 1..];
        rest.find('\'').map(|end| rest[..end].to_string())
    } else {
        None
    };
    quoted
}

fn resolve_stylesheet_specifier(
    record: &SessionRecord,
    importer_path: &str,
    specifier: &str,
) -> Option<String> {
    if specifier.starts_with("http://")
        || specifier.starts_with("https://")
        || specifier.starts_with("//")
        || specifier.starts_with("data:")
    {
        return None;
    }

    if specifier.starts_with('/') {
        return resolve_workspace_stylesheet_path(record, &format!("/workspace{specifier}"));
    }

    if specifier.starts_with('.') {
        return resolve_workspace_stylesheet_path(
            record,
            &normalize_posix_path(&format!("{}/{specifier}", dirname(importer_path))),
        );
    }

    resolve_package_stylesheet_specifier(record, dirname(importer_path), specifier)
}

fn resolve_workspace_stylesheet_path(record: &SessionRecord, base_path: &str) -> Option<String> {
    for candidate in [
        normalize_posix_path(base_path),
        normalize_posix_path(&format!("{base_path}.css")),
        normalize_posix_path(&format!("{base_path}/index.css")),
    ] {
        if record.vfs.read(&candidate).is_some() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_package_stylesheet_specifier(
    record: &SessionRecord,
    importer_dir: &str,
    specifier: &str,
) -> Option<String> {
    let (package_name, remainder) = split_package_specifier(specifier);
    if package_name.is_empty() {
        return None;
    }

    for package_root in node_module_search_roots(importer_dir, &package_name) {
        let manifest = read_package_manifest_at(record, &package_root);
        let subpath = remainder
            .as_deref()
            .map(|value| format!("./{value}"))
            .unwrap_or_else(|| ".".into());

        if let Some(exports_field) = manifest.as_ref().and_then(|value| value.exports.as_ref()) {
            if let Some(target) = resolve_stylesheet_export_target(exports_field, &subpath) {
                if let Some(resolved) = resolve_workspace_stylesheet_path(
                    record,
                    &normalize_posix_path(&format!("{package_root}/{target}")),
                ) {
                    return Some(resolved);
                }
            }
        }

        if remainder.is_none() {
            if let Some(style_entry) = manifest.as_ref().and_then(|value| value.style.as_deref()) {
                if let Some(resolved) = resolve_workspace_stylesheet_path(
                    record,
                    &normalize_posix_path(&format!("{package_root}/{style_entry}")),
                ) {
                    return Some(resolved);
                }
            }
            if let Some(resolved) =
                resolve_workspace_stylesheet_path(record, &format!("{package_root}/index.css"))
            {
                return Some(resolved);
            }
        } else if let Some(remainder) = remainder.as_deref() {
            if let Some(resolved) =
                resolve_workspace_stylesheet_path(record, &format!("{package_root}/{remainder}"))
            {
                return Some(resolved);
            }
        }
    }

    None
}

fn resolve_stylesheet_export_target(exports_field: &JsonValue, subpath: &str) -> Option<String> {
    match exports_field {
        JsonValue::String(target) => {
            if subpath == "." && target.ends_with(".css") {
                Some(target.clone())
            } else {
                None
            }
        }
        JsonValue::Object(map) => {
            if has_conditional_export_keys(map) {
                if subpath == "." {
                    resolve_stylesheet_export_value(exports_field)
                } else {
                    None
                }
            } else if let Some(value) = map.get(subpath) {
                resolve_stylesheet_export_value(value)
            } else if subpath == "." {
                map.get(".").and_then(resolve_stylesheet_export_value)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn resolve_stylesheet_export_value(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(target) => target.ends_with(".css").then(|| target.clone()),
        JsonValue::Object(map) => {
            for condition in ["style", "browser", "default", "import", "module", "require"] {
                if let Some(nested) = map.get(condition) {
                    if let Some(resolved) = resolve_stylesheet_export_value(nested) {
                        return Some(resolved);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn rewrite_svg_document(source: &str, preview_url: &str) -> String {
    rewrite_prefixed_attributes(source, preview_url, &["href", "xlink:href"])
}

fn rewrite_prefixed_attributes(source: &str, preview_url: &str, attributes: &[&str]) -> String {
    let mut rewritten = source.to_string();
    for attribute in attributes {
        for quote in ['"', '\''] {
            let needle = format!("{attribute}={quote}/");
            let replacement = format!("{attribute}={quote}{preview_url}");
            rewritten = rewritten.replace(&needle, &replacement);
        }
    }
    rewritten
}

fn render_preview_app_shell_html(title: &str, entry_url: &str) -> String {
    let entry_url_json = serde_json::to_string(entry_url).unwrap_or_else(|_| "\"\"".into());
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"UTF-8\" /><meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" /><title>{}</title><style>:root{{color-scheme:dark;background:radial-gradient(circle at top, rgba(16, 185, 129, 0.12), transparent 35%),linear-gradient(180deg, #071018 0%, #0b1522 100%);}}*{{box-sizing:border-box;}}html,body{{min-height:100%;}}body{{margin:0;}}#root,#app{{min-height:100vh;}}</style></head><body><div id=\"root\"></div><div id=\"app\"></div><script type=\"module\">globalThis.globalThis ??= globalThis; globalThis.global ??= globalThis; globalThis.process ??= {{ env: {{ NODE_ENV: \"development\" }}, browser: true }}; await import({});</script></body></html>",
        escape_html(title),
        entry_url_json,
    )
}

fn is_preview_guest_component_path(workspace_path: &str) -> bool {
    PREVIEW_GUEST_COMPONENT_CANDIDATES.contains(&workspace_path)
}

fn resolve_preview_guest_stylesheet_urls(
    record: &SessionRecord,
    session_id: &str,
    port: u16,
) -> Vec<String> {
    PREVIEW_GUEST_STYLESHEET_CANDIDATES
        .iter()
        .filter(|candidate| record.vfs.read(candidate).is_some())
        .map(|candidate| build_preview_url_for_workspace_file(session_id, port, candidate, "/workspace"))
        .collect()
}

fn resolve_preview_guest_app_shell_for_path<E: EngineAdapter>(
    host: &RuntimeHostCore<E>,
    context_id: &str,
    record: &SessionRecord,
    session_id: &str,
    port: u16,
    workspace_path: &str,
    client_modules: &[HostRuntimePreviewClientModule],
) -> Option<PreviewGuestAppShell> {
    if record.vfs.read(workspace_path).is_none() {
        return None;
    }
    let react_url = if let Some(url) = find_preview_client_module_url(client_modules, "react") {
        url.to_string()
    } else {
        let react_import = host
            .prepare_runtime_module_import(context_id, "react", Some(workspace_path))
            .ok()?;
        render_preview_module_specifier(
            session_id,
            port,
            &react_import.resolved_module.resolved_specifier,
            "/workspace",
        )
    };
    let react_dom_client_url =
        if let Some(url) = find_preview_client_module_url(client_modules, "react-dom/client") {
        url.to_string()
    } else {
        let react_dom_client_import = host
            .prepare_runtime_module_import(context_id, "react-dom/client", Some(workspace_path))
            .ok()?;
        render_preview_module_specifier(
            session_id,
            port,
            &react_dom_client_import.resolved_module.resolved_specifier,
            "/workspace",
        )
        };
    Some(PreviewGuestAppShell {
        entry_url: build_preview_url_for_workspace_file(session_id, port, workspace_path, "/workspace"),
        stylesheet_urls: resolve_preview_guest_stylesheet_urls(record, session_id, port),
        react_url,
        react_dom_client_url,
    })
}

fn render_preview_error_html(message: &str) -> String {
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"UTF-8\" /><meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" /><title>Preview Error</title></head><body><pre>{}</pre></body></html>",
        escape_html(message),
    )
}

fn render_preview_root_html(
    session_id: &str,
    port: u16,
    model: &HostRuntimePreviewModel,
    client_modules: &[HostRuntimePreviewClientModule],
) -> HostRuntimeDirectHttpResponse {
    let client_script_url =
        find_preview_client_module_url(client_modules, "runtime:preview-client");
    let Some(client_script_url) = client_script_url else {
        return HostRuntimeDirectHttpResponse {
            status: 503,
            headers: BTreeMap::new(),
            text_body: Some(render_preview_error_html(
                "Preview client script is not configured.",
            )),
            bytes_body: None,
        };
    };
    let preview_url = preview_request_url(session_id, port);
    let html = format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"UTF-8\" /><meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" /><title>{}</title><link rel=\"stylesheet\" href=\"{}assets/runtime.css\" /><style>:root{{color-scheme:dark;font-family:\"Iowan Old Style\",\"Palatino Linotype\",serif;background:radial-gradient(circle at top, rgba(245, 158, 11, 0.18), transparent 35%),linear-gradient(160deg, #08111f 0%, #101b2f 50%, #1c2940 100%);color:#f5f7fb;}}*{{box-sizing:border-box;}}html,body,#guest-root{{min-height:100%;}}body{{margin:0;}}</style></head><body><div id=\"guest-root\"></div><script>window.__NODE_IN_NODE_PREVIEW__={{sessionId:{},port:{},bootstrapUrl:{}}};</script><script type=\"module\" src=\"{}\"></script></body></html>",
        escape_html(&model.title),
        preview_url,
        serde_json::to_string(session_id).unwrap_or_else(|_| "\"\"".into()),
        port,
        serde_json::to_string(&format!("{preview_url}__bootstrap.json")).unwrap_or_else(|_| "\"\"".into()),
        client_script_url,
    );
    HostRuntimeDirectHttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        text_body: Some(html),
        bytes_body: None,
    }
}

fn render_preview_guest_app_shell_html(
    title: &str,
    guest_app_shell: &PreviewGuestAppShell,
) -> String {
    let stylesheet_links = guest_app_shell
        .stylesheet_urls
        .iter()
        .map(|href| format!("<link rel=\"stylesheet\" href=\"{}\" />", escape_html(href)))
        .collect::<Vec<_>>()
        .join("");
    let react_url = serde_json::to_string(&guest_app_shell.react_url)
        .unwrap_or_else(|_| "\"\"".into());
    let react_dom_client_url = serde_json::to_string(&guest_app_shell.react_dom_client_url)
        .unwrap_or_else(|_| "\"\"".into());
    let entry_url = serde_json::to_string(&guest_app_shell.entry_url)
        .unwrap_or_else(|_| "\"\"".into());
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"UTF-8\" /><meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" /><title>{}</title>{}</head><body><div id=\"guest-root\"></div><script type=\"module\">globalThis.globalThis ??= globalThis; globalThis.global ??= globalThis; globalThis.process ??= {{ env: {{ NODE_ENV: \"development\" }}, browser: true }}; const React = await import({}); const ReactDOMClient = await import({}); const guestModule = await import({}); const GuestComponent = guestModule.default; const root = document.getElementById(\"guest-root\"); if (root && typeof GuestComponent === \"function\") {{ ReactDOMClient.createRoot(root).render(React.createElement(GuestComponent)); }}</script></body></html>",
        escape_html(title),
        stylesheet_links,
        react_url,
        react_dom_client_url,
        entry_url,
    )
}

fn render_preview_json_response(value: JsonValue) -> HostRuntimeDirectHttpResponse {
    HostRuntimeDirectHttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        text_body: Some(value.to_string()),
        bytes_body: None,
    }
}

fn render_preview_ready_event_json(preview: &HostRuntimePreviewStateReport) -> JsonValue {
    serde_json::json!({
        "type": "preview.ready",
        "sessionId": extract_session_id_from_preview_url(&preview.url).unwrap_or_default(),
        "pid": 0,
        "port": preview.port.port,
        "url": preview.url,
        "model": render_preview_model_json(&preview.model),
        "host": render_host_summary_json(&preview.host),
        "run": render_run_plan_json_value(&preview.run),
        "hostFiles": render_host_file_summary_json(&preview.host_files),
    })
}

fn render_preview_bootstrap_json(
    record: &SessionRecord,
    session: &HostSessionStateReport,
    preview: &HostRuntimePreviewStateReport,
    request_hint: &PreviewRequestHint,
    hydrated_files: &[WorkspaceFilePayload],
) -> JsonValue {
    serde_json::json!({
        "preview": render_preview_ready_event_json(preview),
        "workspace": render_session_snapshot_json(session),
        "files": render_preview_file_index_json(&session.session_id, preview, &session.host_files.index),
        "selectedFile": render_preview_selected_file_json(record, &session.session_id, preview),
        "diagnostics": render_preview_diagnostics_json(session, preview, request_hint, hydrated_files),
    })
}

fn render_preview_selected_file_json(
    record: &SessionRecord,
    session_id: &str,
    preview: &HostRuntimePreviewStateReport,
) -> JsonValue {
    let document_root = resolve_preview_document_root(preview);
    let mut text_files = record
        .vfs
        .files()
        .filter(|file| file.is_text)
        .collect::<Vec<_>>();
    text_files.sort_by(|left, right| left.path.cmp(&right.path));
    let preferred = text_files
        .iter()
        .copied()
        .find(|file| file.path.ends_with("/package.json"))
        .or_else(|| text_files.iter().copied().find(|file| file.path.contains("/src/")))
        .or_else(|| text_files.iter().copied().find(|file| file.path.contains("/app/")))
        .or_else(|| text_files.first().copied());

    match preferred {
        Some(file) => serde_json::json!({
            "path": file.path,
            "size": file.bytes.len(),
            "contentType": guess_preview_file_content_type(&file.path),
            "isText": file.is_text,
            "url": format!("{}files{}", preview.url, file.path.replace("/workspace", "")),
            "previewUrl": build_preview_url_for_workspace_file(
                session_id,
                preview.port.port,
                &file.path,
                &document_root,
            ),
            "content": String::from_utf8_lossy(&file.bytes),
        }),
        None => JsonValue::Null,
    }
}

fn render_session_snapshot_json(session: &HostSessionStateReport) -> JsonValue {
    serde_json::json!({
        "sessionId": session.session_id,
        "state": render_session_state_label(&session.state),
        "revision": session.revision,
        "workspaceRoot": session.workspace_root,
        "archive": {
            "fileName": session.archive.file_name,
            "fileCount": session.archive.file_count,
            "directoryCount": session.archive.directory_count,
            "rootPrefix": session.archive.root_prefix,
            "entries": session.archive_entries.iter().map(|entry| serde_json::json!({
                "path": entry.path,
                "size": entry.size,
                "kind": match entry.kind {
                    ArchiveEntryKind::File => "file",
                    ArchiveEntryKind::Directory => "dir",
                },
            })).collect::<Vec<_>>(),
        },
        "packageJson": session.package_json.as_ref().map(|package_json| serde_json::json!({
            "name": package_json.name,
            "scripts": package_json.scripts,
            "dependencies": package_json.dependencies,
            "devDependencies": package_json.dev_dependencies,
        })),
        "suggestedRunRequest": session.suggested_run_request.as_ref().map(|request| serde_json::json!({
            "cwd": request.cwd,
            "command": request.command,
            "args": request.args,
            "env": request.env,
        })),
        "capabilities": {
            "detectedReact": session.capabilities.detected_react,
        },
    })
}

fn suggested_run_request(
    package_json: Option<&PackageJsonSummary>,
    workspace_root: &str,
) -> Option<RunRequest> {
    let scripts = &package_json?.scripts;
    const SUGGESTED_RUN_SCRIPT_NAMES: &[&str] = &["dev", "start"];

    SUGGESTED_RUN_SCRIPT_NAMES
        .iter()
        .find(|script_name| {
            scripts
                .get(**script_name)
                .is_some_and(|script| !script.trim().is_empty())
        })
        .map(|script_name| {
            RunRequest::new(
                workspace_root.to_string(),
                String::from("npm"),
                vec![String::from("run"), String::from(*script_name)],
            )
        })
}

fn render_preview_file_index_json(
    session_id: &str,
    preview: &HostRuntimePreviewStateReport,
    files: &[WorkspaceFileSummary],
) -> JsonValue {
    let document_root = resolve_preview_document_root(preview);
    JsonValue::Array(
        files.iter()
            .filter(|file| file.is_text)
            .map(|file| {
                let content_type = guess_preview_file_content_type(&file.path);
                serde_json::json!({
                    "path": file.path,
                    "size": file.size,
                    "contentType": content_type,
                    "isText": file.is_text,
                    "url": format!("{}files{}", preview.url, file.path.replace("/workspace", "")),
                    "previewUrl": build_preview_url_for_workspace_file(
                        session_id,
                        preview.port.port,
                        &file.path,
                        &document_root,
                    ),
                })
            })
            .collect(),
    )
}

fn render_preview_diagnostics_json(
    session: &HostSessionStateReport,
    preview: &HostRuntimePreviewStateReport,
    request_hint: &PreviewRequestHint,
    hydrated_files: &[WorkspaceFilePayload],
) -> JsonValue {
    let mut hydrated_paths = preview
        .root_response_descriptor
        .hydrate_paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    hydrated_paths.extend(hydrated_files.iter().map(|file| file.path.clone()));

    serde_json::json!({
        "sessionId": session.session_id,
        "pid": 0,
        "port": preview.port.port,
        "url": preview.url,
        "model": render_preview_model_json(&preview.model),
        "session": render_session_snapshot_json(session),
        "rootRequestHint": render_preview_request_hint_json(&preview.root_request_hint),
        "requestHint": render_preview_request_hint_json(request_hint),
        "fileCount": session.host_files.count,
        "hydratedFileCount": hydrated_paths.len(),
        "hydratedPaths": hydrated_paths.into_iter().collect::<Vec<_>>(),
        "host": render_host_summary_json(&preview.host),
        "run": render_run_plan_json_value(&preview.run),
        "hostFiles": render_host_file_summary_json(&preview.host_files),
    })
}

fn render_preview_model_json(model: &HostRuntimePreviewModel) -> JsonValue {
    serde_json::json!({
        "title": model.title,
        "summary": model.summary,
        "cwd": model.cwd,
        "command": model.command,
        "highlights": model.highlights,
    })
}

fn render_host_summary_json(summary: &HostBootstrapSummary) -> JsonValue {
    serde_json::json!({
        "engineName": summary.engine_name,
        "supportsInterrupts": summary.supports_interrupts,
        "supportsModuleLoader": summary.supports_module_loader,
        "workspaceRoot": summary.workspace_root,
    })
}

fn render_run_plan_json_value(plan: &RunPlan) -> JsonValue {
    serde_json::json!({
        "cwd": plan.cwd,
        "entrypoint": plan.entrypoint,
        "commandLine": plan.command_line,
        "envCount": plan.env_count,
        "commandKind": match plan.command_kind {
            crate::protocol::RunCommandKind::NpmScript => "npm-script",
            crate::protocol::RunCommandKind::NodeEntrypoint => "node-entrypoint",
        },
        "resolvedScript": plan.resolved_script,
    })
}

fn render_host_file_summary_json(summary: &HostWorkspaceFileIndexSummary) -> JsonValue {
    serde_json::json!({
        "count": summary.count,
        "samplePath": summary.sample_path,
        "sampleSize": summary.sample_size,
    })
}

fn render_preview_request_hint_json(hint: &PreviewRequestHint) -> JsonValue {
    serde_json::json!({
        "kind": match hint.kind {
            PreviewRequestKind::RootDocument => "root-document",
            PreviewRequestKind::RootEntry => "root-entry",
            PreviewRequestKind::FallbackRoot => "fallback-root",
            PreviewRequestKind::BootstrapState => "bootstrap-state",
            PreviewRequestKind::RuntimeState => "runtime-state",
            PreviewRequestKind::WorkspaceState => "workspace-state",
            PreviewRequestKind::FileIndex => "file-index",
            PreviewRequestKind::DiagnosticsState => "diagnostics-state",
            PreviewRequestKind::RuntimeStylesheet => "runtime-stylesheet",
            PreviewRequestKind::WorkspaceFile => "workspace-file",
            PreviewRequestKind::WorkspaceAsset => "workspace-asset",
            PreviewRequestKind::NotFound => "not-found",
        },
        "workspacePath": hint.workspace_path,
        "documentRoot": hint.document_root,
        "hydratePaths": hint.hydrate_paths,
    })
}

fn render_session_state_label(state: &SessionState) -> &'static str {
    match state {
        SessionState::Booting => "booting",
        SessionState::Mounted => "mounted",
        SessionState::Running => "running",
        SessionState::Stopped => "stopped",
        SessionState::Errored => "errored",
    }
}

fn resolve_preview_document_root(preview: &HostRuntimePreviewStateReport) -> String {
    if matches!(preview.root_request_hint.kind, PreviewRequestKind::RootDocument) {
        preview
            .root_request_hint
            .document_root
            .clone()
            .unwrap_or_else(|| String::from("/workspace"))
    } else {
        String::from("/workspace")
    }
}

fn guess_preview_file_content_type(path: &str) -> String {
    if path.ends_with(".html") {
        return String::from("text/html; charset=utf-8");
    }
    if path.ends_with(".css") {
        return String::from("text/css; charset=utf-8");
    }
    if path.ends_with(".json") {
        return String::from("application/json; charset=utf-8");
    }
    if path.ends_with(".js")
        || path.ends_with(".mjs")
        || path.ends_with(".cjs")
        || path.ends_with(".mts")
        || path.ends_with(".cts")
    {
        return String::from("text/javascript; charset=utf-8");
    }
    if path.ends_with(".ts") || path.ends_with(".tsx") || path.ends_with(".jsx") {
        return String::from("text/plain; charset=utf-8");
    }
    if path.ends_with(".md") {
        return String::from("text/markdown; charset=utf-8");
    }
    if path.ends_with(".svg") {
        return String::from("image/svg+xml; charset=utf-8");
    }
    if path.ends_with(".png") {
        return String::from("image/png");
    }
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        return String::from("image/jpeg");
    }
    if path.ends_with(".gif") {
        return String::from("image/gif");
    }
    if path.ends_with(".webp") {
        return String::from("image/webp");
    }
    if path.ends_with(".ico") {
        return String::from("image/x-icon");
    }
    if path.ends_with(".woff") {
        return String::from("font/woff");
    }
    if path.ends_with(".woff2") {
        return String::from("font/woff2");
    }
    String::from("application/octet-stream")
}

fn render_runtime_stylesheet() -> String {
    String::from(
        r#"
    .guest-shell {
      position: relative;
    }

    .guest-shell::after {
      content: "";
      position: absolute;
      inset: auto 0 -40px auto;
      width: 220px;
      height: 220px;
      border-radius: 999px;
      background: radial-gradient(circle, rgba(249, 115, 22, 0.28), transparent 70%);
      filter: blur(10px);
      pointer-events: none;
    }

    .guest-columns {
      display: grid;
      grid-template-columns: 1.2fr 0.8fr;
      gap: 18px;
    }

    .guest-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
      gap: 12px;
      margin: 20px 0;
    }

    .guest-card,
    .guest-metric,
    .guest-console {
      border-radius: 18px;
      border: 1px solid rgba(148, 163, 184, 0.18);
      background: rgba(8, 15, 28, 0.72);
      box-shadow: 0 22px 60px rgba(15, 23, 42, 0.25);
      backdrop-filter: blur(14px);
    }

    .guest-card {
      padding: 18px 20px;
    }

    .guest-card h4 {
      margin: 0 0 12px;
      font-size: 0.95rem;
    }

    .guest-metric {
      padding: 14px 16px;
    }

    .guest-metric span {
      display: block;
      font-size: 0.72rem;
      opacity: 0.7;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      margin-bottom: 6px;
    }

    .guest-metric strong {
      font-size: 0.95rem;
      line-height: 1.4;
      word-break: break-word;
    }

    .guest-badge {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 6px 12px;
      border-radius: 999px;
      background: rgba(15, 23, 42, 0.55);
      border: 1px solid rgba(148, 163, 184, 0.22);
      font-size: 0.72rem;
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }

    .guest-console {
      margin: 18px 0;
      padding: 14px 16px;
      font-family: "SFMono-Regular", "Menlo", monospace;
      font-size: 0.85rem;
    }

    .guest-console div {
      display: flex;
      gap: 10px;
      align-items: baseline;
      white-space: pre-wrap;
      word-break: break-word;
    }

    .guest-console span {
      color: #f59e0b;
    }

    .guest-list {
      display: grid;
      gap: 10px;
      padding: 0;
      margin: 0;
      list-style: none;
    }

    .guest-list li {
      display: grid;
      gap: 4px;
    }

    .guest-list code {
      font-size: 0.74rem;
      color: #f8c56d;
      word-break: break-all;
    }

    .guest-list span {
      color: rgba(226, 232, 240, 0.85);
      word-break: break-word;
    }

    .guest-source {
      overflow: auto;
      padding: 16px;
      border-radius: 14px;
      background: rgba(15, 23, 42, 0.75);
      border: 1px solid rgba(148, 163, 184, 0.16);
      font-size: 0.8rem;
      line-height: 1.55;
    }

    @media (max-width: 760px) {
      .guest-columns {
        grid-template-columns: 1fr;
      }
    }
  "#,
    )
}

fn extract_session_id_from_preview_url(url: &str) -> Option<String> {
    let trimmed = url.trim_matches('/');
    let mut segments = trimmed.split('/');
    match (segments.next(), segments.next(), segments.next()) {
        (Some("preview"), Some(session_id), Some(_port)) => Some(session_id.to_string()),
        _ => None,
    }
}

fn build_preview_direct_workspace_file_response(
    record: &SessionRecord,
    session_id: &str,
    port: u16,
    file: &VirtualFile,
    descriptor: &PreviewResponseDescriptor,
    transform_kind: Option<&HostRuntimePreviewTransformKind>,
    preview_url: Option<&str>,
) -> HostRuntimeDirectHttpResponse {
    let text_body = if file.is_text {
        let text = String::from_utf8_lossy(&file.bytes).into_owned();
        Some(match transform_kind {
            Some(HostRuntimePreviewTransformKind::HtmlDocument) => {
                rewrite_html_document(&text, preview_url.unwrap_or("/"))
            }
            Some(HostRuntimePreviewTransformKind::Stylesheet) => {
                rewrite_stylesheet_document(
                    record,
                    &text,
                    &file.path,
                    session_id,
                    port,
                    preview_url.unwrap_or("/"),
                )
            }
            Some(HostRuntimePreviewTransformKind::SvgDocument) => {
                rewrite_svg_document(&text, preview_url.unwrap_or("/"))
            }
            _ => text,
        })
    } else {
        None
    };
    let bytes_body = if file.is_text {
        None
    } else {
        Some(file.bytes.clone())
    };
    HostRuntimeDirectHttpResponse {
        status: descriptor.status_code,
        headers: BTreeMap::new(),
        text_body,
        bytes_body,
    }
}

fn apply_direct_preview_response_metadata(
    mut response: HostRuntimeDirectHttpResponse,
    descriptor: &PreviewResponseDescriptor,
) -> HostRuntimeDirectHttpResponse {
    if let Some(content_type) = &descriptor.content_type {
        response
            .headers
            .insert(String::from("content-type"), content_type.clone());
    }
    response
        .headers
        .insert(String::from("cache-control"), String::from("no-store"));
    if !descriptor.allow_methods.is_empty() {
        response.headers.insert(
            String::from("allow"),
            descriptor.allow_methods.join(", "),
        );
    }
    if descriptor.omit_body {
        response.text_body = Some(String::new());
        response.bytes_body = None;
    }
    response
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
            HostRuntimeBuiltinSpec {
                name: "perf_hooks".into(),
                globals: vec!["performance".into()],
                modules: vec!["node:perf_hooks".into()],
                command_prefixes: Vec::new(),
            },
            HostRuntimeBuiltinSpec {
                name: "module".into(),
                globals: Vec::new(),
                modules: vec!["node:module".into()],
                command_prefixes: Vec::new(),
            },
            HostRuntimeBuiltinSpec {
                name: "inspector".into(),
                globals: Vec::new(),
                modules: vec!["node:inspector".into()],
                command_prefixes: Vec::new(),
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
    let mut modules = BUILTIN_BOOTSTRAP_MODULE_SPECS
        .iter()
        .map(|module| HostRuntimeBootstrapModule {
            specifier: module.specifier.into(),
            source: module.source.into(),
        })
        .collect::<Vec<_>>();

    modules.extend(
        BROWSER_CLI_SHIMS
            .iter()
            .filter(|shim| shim.specifier == bindings.entrypoint)
            .map(|shim| HostRuntimeBootstrapModule {
                specifier: shim.specifier.into(),
                source: render_browser_cli_shim_source(shim),
            }),
    );
    modules.push(HostRuntimeBootstrapModule {
        specifier: bootstrap_specifier.clone(),
        source: format!(
            r#"import process from "node:process";
import {{ performance }} from "node:perf_hooks";
import {{ Buffer }} from "node:buffer";
import consoleValue from "node:console";
import {{ setTimeout, clearTimeout, setInterval, clearInterval }} from "node:timers";

globalThis.process = process;
globalThis.performance = performance;
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
    });

    HostRuntimeBootstrapPlan {
        context_id: bindings.context_id.clone(),
        engine_name: bindings.engine_name.clone(),
        entrypoint: bindings.entrypoint.clone(),
        bootstrap_specifier,
        modules,
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
        client_modules: request.client_modules.clone(),
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
        PreviewRequestKind::BootstrapState => PreviewResponseKind::BootstrapState,
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
        | PreviewResponseKind::BootstrapState
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

fn resolve_preview_module_plan_workspace_path<'a>(
    request_hint: &'a PreviewRequestHint,
    response_descriptor: &'a PreviewResponseDescriptor,
) -> Option<&'a str> {
    response_descriptor
        .workspace_path
        .as_deref()
        .or(request_hint.workspace_path.as_deref())
}

fn determine_runtime_preview_transform_kind(
    request_hint: &PreviewRequestHint,
    response_descriptor: &PreviewResponseDescriptor,
    module_plan: Option<&HostRuntimePreviewModulePlan>,
) -> Option<crate::protocol::HostRuntimePreviewTransformKind> {
    if module_plan.is_some() && matches!(response_descriptor.kind, PreviewResponseKind::WorkspaceAsset)
    {
        return Some(crate::protocol::HostRuntimePreviewTransformKind::Module);
    }

    let workspace_path = response_descriptor
        .workspace_path
        .as_deref()
        .or(request_hint.workspace_path.as_deref())?;

    let normalized = workspace_path.to_ascii_lowercase();
    if normalized.ends_with(".html") {
        return Some(crate::protocol::HostRuntimePreviewTransformKind::HtmlDocument);
    }
    if normalized.ends_with(".css") {
        return Some(crate::protocol::HostRuntimePreviewTransformKind::Stylesheet);
    }
    if normalized.ends_with(".svg") {
        return Some(crate::protocol::HostRuntimePreviewTransformKind::SvgDocument);
    }
    if normalized.ends_with(".png")
        || normalized.ends_with(".jpg")
        || normalized.ends_with(".jpeg")
        || normalized.ends_with(".gif")
        || normalized.ends_with(".webp")
        || normalized.ends_with(".ico")
        || normalized.ends_with(".woff")
        || normalized.ends_with(".woff2")
    {
        return Some(crate::protocol::HostRuntimePreviewTransformKind::Binary);
    }

    Some(crate::protocol::HostRuntimePreviewTransformKind::PlainText)
}

fn build_runtime_preview_render_plan(
    request_hint: &PreviewRequestHint,
    response_descriptor: &PreviewResponseDescriptor,
) -> Option<crate::protocol::HostRuntimePreviewRenderPlan> {
    match response_descriptor.kind {
        PreviewResponseKind::WorkspaceDocument
        | PreviewResponseKind::WorkspaceFile
        | PreviewResponseKind::WorkspaceAsset => Some(crate::protocol::HostRuntimePreviewRenderPlan {
            kind: crate::protocol::HostRuntimePreviewRenderKind::WorkspaceFile,
            workspace_path: response_descriptor
                .workspace_path
                .clone()
                .or_else(|| request_hint.workspace_path.clone()),
            document_root: response_descriptor
                .document_root
                .clone()
                .or_else(|| request_hint.document_root.clone()),
        }),
        PreviewResponseKind::AppShell => Some(crate::protocol::HostRuntimePreviewRenderPlan {
            kind: crate::protocol::HostRuntimePreviewRenderKind::AppShell,
            workspace_path: response_descriptor
                .workspace_path
                .clone()
                .or_else(|| request_hint.workspace_path.clone()),
            document_root: Some(String::from("/workspace")),
        }),
        PreviewResponseKind::HostManagedFallback => {
            Some(crate::protocol::HostRuntimePreviewRenderPlan {
                kind: crate::protocol::HostRuntimePreviewRenderKind::HostManagedFallback,
                workspace_path: None,
                document_root: None,
            })
        }
        _ => None,
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
