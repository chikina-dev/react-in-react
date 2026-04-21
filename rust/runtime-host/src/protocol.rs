use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveStats {
    pub file_name: String,
    pub file_count: usize,
    pub directory_count: usize,
    pub root_prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntrySummary {
    pub path: String,
    pub size: usize,
    pub kind: ArchiveEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageJsonSummary {
    pub name: Option<String>,
    pub scripts: BTreeMap<String, String>,
    pub dependencies: Vec<String>,
    pub dev_dependencies: Vec<String>,
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
    pub revision: u64,
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
pub enum RunCommandKind {
    NpmScript,
    NodeEntrypoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub cwd: String,
    pub entrypoint: String,
    pub command_line: String,
    pub env_count: usize,
    pub command_kind: RunCommandKind,
    pub resolved_script: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBootstrapSummary {
    pub engine_name: String,
    pub supports_interrupts: bool,
    pub supports_module_loader: bool,
    pub workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostProcessInfo {
    pub cwd: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub exec_path: String,
    pub platform: String,
    pub entrypoint: String,
    pub command_line: String,
    pub command_kind: RunCommandKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeContext {
    pub context_id: String,
    pub session_id: String,
    pub process: HostProcessInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeBuiltinSpec {
    pub name: String,
    pub globals: Vec<String>,
    pub modules: Vec<String>,
    pub command_prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeBindings {
    pub context_id: String,
    pub engine_name: String,
    pub entrypoint: String,
    pub globals: Vec<String>,
    pub builtins: Vec<HostRuntimeBuiltinSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeBootstrapModule {
    pub specifier: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeBootstrapPlan {
    pub context_id: String,
    pub engine_name: String,
    pub entrypoint: String,
    pub bootstrap_specifier: String,
    pub modules: Vec<HostRuntimeBootstrapModule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeEngineBoot {
    pub plan: HostRuntimeBootstrapPlan,
    pub loader_plan: HostRuntimeModuleLoaderPlan,
    pub result_summary: String,
    pub pending_jobs: usize,
    pub drained_jobs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeModuleRecord {
    pub specifier: String,
    pub source_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeModuleSource {
    pub specifier: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeModuleKind {
    Registered,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeModuleFormat {
    Module,
    CommonJs,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeResolvedModule {
    pub requested_specifier: String,
    pub resolved_specifier: String,
    pub kind: HostRuntimeModuleKind,
    pub format: HostRuntimeModuleFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeLoadedModule {
    pub resolved_specifier: String,
    pub kind: HostRuntimeModuleKind,
    pub format: HostRuntimeModuleFormat,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeModuleImportPlan {
    pub request_specifier: String,
    pub importer: Option<String>,
    pub resolved_module: HostRuntimeResolvedModule,
    pub loaded_module: HostRuntimeLoadedModule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeModuleLoaderPlan {
    pub context_id: String,
    pub engine_name: String,
    pub cwd: String,
    pub entrypoint: String,
    pub workspace_root: String,
    pub entry_module: HostRuntimeResolvedModule,
    pub registered_specifiers: Vec<String>,
    pub node_module_search_roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeStdioStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimePortProtocol {
    Http,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeTimerKind {
    Timeout,
    Interval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeTimer {
    pub timer_id: String,
    pub kind: HostRuntimeTimerKind,
    pub delay_ms: u64,
    pub due_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeIdleReport {
    pub turns: usize,
    pub drained_jobs: usize,
    pub fired_timers: usize,
    pub now_ms: u64,
    pub pending_jobs: usize,
    pub pending_timers: usize,
    pub exited: bool,
    pub exit_code: Option<i32>,
    pub reached_turn_limit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeStartupReport {
    pub boot: HostRuntimeEngineBoot,
    pub entry_import_plan: HostRuntimeModuleImportPlan,
    pub idle: HostRuntimeIdleReport,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimePreviewLaunchReport {
    pub startup: HostRuntimeStartupReport,
    pub server: Option<HostRuntimeHttpServer>,
    pub port: Option<HostRuntimePort>,
    pub root_request: Option<HostRuntimeHttpRequest>,
    pub root_request_hint: Option<PreviewRequestHint>,
    pub root_response_descriptor: Option<PreviewResponseDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostWorkspaceFileIndexSummary {
    pub count: usize,
    pub index: Vec<WorkspaceFileSummary>,
    pub sample_path: Option<String>,
    pub sample_size: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSessionStateReport {
    pub session_id: String,
    pub state: SessionState,
    pub revision: u64,
    pub workspace_root: String,
    pub archive: ArchiveStats,
    pub archive_entries: Vec<ArchiveEntrySummary>,
    pub package_json: Option<PackageJsonSummary>,
    pub capabilities: CapabilityMatrix,
    pub host_files: HostWorkspaceFileIndexSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimePreviewStateReport {
    pub port: HostRuntimePort,
    pub root_request: HostRuntimeHttpRequest,
    pub root_request_hint: PreviewRequestHint,
    pub root_response_descriptor: PreviewResponseDescriptor,
    pub host: HostBootstrapSummary,
    pub run: RunPlan,
    pub host_files: HostWorkspaceFileIndexSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeStateReport {
    pub session: HostSessionStateReport,
    pub preview: Option<HostRuntimePreviewStateReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeLaunchReport {
    pub boot_summary: HostBootstrapSummary,
    pub run_plan: RunPlan,
    pub runtime_context: HostRuntimeContext,
    pub engine_context: crate::engine::EngineContextSnapshot,
    pub bindings: HostRuntimeBindings,
    pub bootstrap_plan: HostRuntimeBootstrapPlan,
    pub preview_launch: HostRuntimePreviewLaunchReport,
    pub state: HostRuntimeStateReport,
    pub startup_logs: Vec<String>,
    pub events: Vec<HostRuntimeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimePreviewRequestReport {
    pub server: HostRuntimeHttpServer,
    pub port: HostRuntimePort,
    pub request: HostRuntimeHttpRequest,
    pub request_hint: PreviewRequestHint,
    pub response_descriptor: PreviewResponseDescriptor,
    pub hydration_paths: Vec<String>,
    pub hydrated_files: Vec<WorkspaceFilePayload>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeShutdownReport {
    pub context_id: String,
    pub session_id: String,
    pub exit_code: i32,
    pub closed_ports: Vec<HostRuntimePort>,
    pub closed_servers: Vec<HostRuntimeHttpServer>,
    pub events: Vec<HostRuntimeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFilePayload {
    pub path: String,
    pub size: usize,
    pub is_text: bool,
    pub text_content: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimePort {
    pub port: u16,
    pub protocol: HostRuntimePortProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeHttpRequest {
    pub port: u16,
    pub method: String,
    pub relative_path: String,
    pub search: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeHttpServerKind {
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeHttpServer {
    pub port: HostRuntimePort,
    pub kind: HostRuntimeHttpServerKind,
    pub cwd: String,
    pub entrypoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeEvent {
    Stdout {
        chunk: String,
    },
    Stderr {
        chunk: String,
    },
    Console {
        level: HostRuntimeConsoleLevel,
        line: String,
    },
    ProcessExit {
        code: i32,
    },
    PortListen {
        port: HostRuntimePort,
    },
    PortClose {
        port: u16,
    },
    WorkspaceChange {
        entry: WorkspaceEntrySummary,
        revision: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileSummary {
    pub path: String,
    pub size: usize,
    pub is_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntrySummary {
    pub path: String,
    pub kind: WorkspaceEntryKind,
    pub size: usize,
    pub is_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostFsCommand {
    Exists {
        cwd: String,
        path: String,
    },
    Stat {
        cwd: String,
        path: String,
    },
    ReadDir {
        cwd: String,
        path: String,
    },
    ReadFile {
        cwd: String,
        path: String,
    },
    CreateDirAll {
        cwd: String,
        path: String,
    },
    WriteFile {
        cwd: String,
        path: String,
        bytes: Vec<u8>,
        is_text: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostContextFsCommand {
    Exists {
        path: String,
    },
    Stat {
        path: String,
    },
    ReadDir {
        path: String,
    },
    ReadFile {
        path: String,
    },
    CreateDirAll {
        path: String,
    },
    WriteFile {
        path: String,
        bytes: Vec<u8>,
        is_text: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeCommand {
    DescribeBindings,
    DescribeBootstrap,
    DescribeState,
    BootEngine,
    Startup {
        max_turns: usize,
    },
    LaunchPreview {
        max_turns: usize,
        port: Option<u16>,
    },
    PreviewRequest {
        request: HostRuntimeHttpRequest,
    },
    Shutdown {
        code: i32,
    },
    RunUntilIdle {
        max_turns: usize,
    },
    DescribeModuleLoader,
    DescribeModules,
    ReadModule {
        specifier: String,
    },
    PrepareModuleImport {
        specifier: String,
        importer: Option<String>,
    },
    ResolveModule {
        specifier: String,
        importer: Option<String>,
    },
    LoadModule {
        resolved_specifier: String,
    },
    StdioWrite {
        stream: HostRuntimeStdioStream,
        chunk: String,
    },
    ConsoleEmit {
        level: HostRuntimeConsoleLevel,
        values: Vec<String>,
    },
    DrainEvents,
    PortListen {
        port: Option<u16>,
        protocol: HostRuntimePortProtocol,
    },
    PortClose {
        port: u16,
    },
    PortList,
    HttpServePreview {
        port: Option<u16>,
    },
    HttpCloseServer {
        port: u16,
    },
    HttpListServers,
    HttpResolvePreview {
        request: HostRuntimeHttpRequest,
    },
    TimerSchedule {
        delay_ms: u64,
        repeat: bool,
    },
    TimerClear {
        timer_id: String,
    },
    TimerList,
    TimerAdvance {
        elapsed_ms: u64,
    },
    ProcessInfo,
    ProcessStatus,
    ProcessCwd,
    ProcessArgv,
    ProcessEnv,
    ProcessExit {
        code: i32,
    },
    ProcessChdir {
        path: String,
    },
    PathResolve {
        segments: Vec<String>,
    },
    PathJoin {
        segments: Vec<String>,
    },
    PathDirname {
        path: String,
    },
    PathBasename {
        path: String,
    },
    PathExtname {
        path: String,
    },
    PathNormalize {
        path: String,
    },
    Fs(HostContextFsCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRuntimeResponse {
    Bindings(HostRuntimeBindings),
    BootstrapPlan(HostRuntimeBootstrapPlan),
    StateReport(HostRuntimeStateReport),
    EngineBoot(HostRuntimeEngineBoot),
    StartupReport(HostRuntimeStartupReport),
    PreviewLaunchReport(HostRuntimePreviewLaunchReport),
    PreviewRequestReport(HostRuntimePreviewRequestReport),
    ShutdownReport(HostRuntimeShutdownReport),
    IdleReport(HostRuntimeIdleReport),
    ModuleLoaderPlan(HostRuntimeModuleLoaderPlan),
    ModuleList {
        modules: Vec<HostRuntimeModuleRecord>,
    },
    ModuleSource(HostRuntimeModuleSource),
    ModuleImportPlan {
        plan: HostRuntimeModuleImportPlan,
    },
    ModuleResolved {
        module: HostRuntimeResolvedModule,
    },
    ModuleLoaded {
        module: HostRuntimeLoadedModule,
    },
    EventQueued {
        queue_len: usize,
    },
    RuntimeEvents {
        events: Vec<HostRuntimeEvent>,
    },
    PortListening {
        port: HostRuntimePort,
    },
    PortClosed {
        port: u16,
        existed: bool,
    },
    PortList {
        ports: Vec<HostRuntimePort>,
    },
    HttpServerListening {
        server: HostRuntimeHttpServer,
    },
    HttpServerClosed {
        port: u16,
        existed: bool,
    },
    HttpServerList {
        servers: Vec<HostRuntimeHttpServer>,
    },
    PreviewRequestResolved {
        server: HostRuntimeHttpServer,
        port: HostRuntimePort,
        request: HostRuntimeHttpRequest,
        request_hint: PreviewRequestHint,
        response_descriptor: PreviewResponseDescriptor,
    },
    TimerScheduled {
        timer: HostRuntimeTimer,
    },
    TimerCleared {
        timer_id: String,
        existed: bool,
    },
    TimerList {
        now_ms: u64,
        timers: Vec<HostRuntimeTimer>,
    },
    TimerFired {
        now_ms: u64,
        timers: Vec<HostRuntimeTimer>,
    },
    ProcessInfo(HostProcessInfo),
    ProcessStatus {
        exited: bool,
        exit_code: Option<i32>,
    },
    ProcessCwd {
        cwd: String,
    },
    ProcessArgv {
        argv: Vec<String>,
    },
    ProcessEnv {
        env: BTreeMap<String, String>,
    },
    PathValue {
        value: String,
    },
    Fs(HostFsResponse),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostFsResponse {
    Exists {
        path: String,
        exists: bool,
    },
    Entry(WorkspaceEntrySummary),
    DirectoryEntries(Vec<WorkspaceEntrySummary>),
    File {
        path: String,
        size: usize,
        is_text: bool,
        text_content: Option<String>,
        bytes: Vec<u8>,
    },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewResponseKind {
    WorkspaceDocument,
    AppShell,
    HostManagedFallback,
    RuntimeState,
    WorkspaceState,
    FileIndex,
    DiagnosticsState,
    RuntimeStylesheet,
    WorkspaceFile,
    WorkspaceAsset,
    MethodNotAllowed,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewResponseDescriptor {
    pub kind: PreviewResponseKind,
    pub workspace_path: Option<String>,
    pub document_root: Option<String>,
    pub hydrate_paths: Vec<String>,
    pub status_code: u16,
    pub content_type: Option<String>,
    pub allow_methods: Vec<String>,
    pub omit_body: bool,
}
