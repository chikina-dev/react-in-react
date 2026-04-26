export type SessionId = string;
export type ProcessId = number;
export type SessionState = "booting" | "mounted" | "running" | "stopped" | "errored";

export type RunRequest = {
  cwd: string;
  command: string;
  args: string[];
  env?: Record<string, string>;
};

export type ArchiveEntry = {
  path: string;
  size: number;
  kind: "file" | "dir";
};

export type ArchiveSummary = {
  fileName: string;
  fileCount: number;
  directoryCount: number;
  entries: ArchiveEntry[];
  rootPrefix: string | null;
};

export type PackageJsonSummary = {
  name?: string;
  scripts: Record<string, string>;
  dependencies: string[];
  devDependencies: string[];
};

export type SessionSnapshot = {
  sessionId: SessionId;
  state: SessionState;
  revision: number;
  workspaceRoot: string;
  archive: ArchiveSummary;
  packageJson: PackageJsonSummary | null;
  suggestedRunRequest?: RunRequest | null;
  capabilities: {
    detectedReact: boolean;
  };
};

export type PreviewModel = {
  title: string;
  summary: string;
  cwd: string;
  command: string;
  highlights: string[];
};

export type PreviewRunPlan = {
  cwd: string;
  entrypoint: string;
  commandLine: string;
  envCount: number;
  commandKind: "npm-script" | "node-entrypoint";
  resolvedScript: string | null;
};

export type PreviewHostSummary = {
  engineName: string;
  supportsInterrupts: boolean;
  supportsModuleLoader: boolean;
  workspaceRoot: string;
};

export type PreviewHostFileSummary = {
  count: number;
  samplePath: string | null;
  sampleSize: number | null;
};

export type PreviewWorkspaceFile = {
  path: string;
  size: number;
  contentType: string;
  isText: boolean;
  url: string;
  previewUrl: string;
};

export type PreviewSelectedFile = PreviewWorkspaceFile & {
  content: string;
};

export type VirtualHttpRequest = {
  sessionId: SessionId;
  port: number;
  method: string;
  pathname: string;
  search: string;
  headers: Record<string, string>;
};

export type VirtualHttpResponse = {
  status: number;
  headers: Record<string, string>;
  body: string | Uint8Array;
};

export type RuntimeError = {
  code: string;
  message: string;
  detail?: string;
  path?: string;
  pid?: number;
};

export type PreviewDiagnostics = {
  sessionId: SessionId;
  pid: number;
  port: number;
  url: string;
  model: PreviewModel;
  session: SessionSnapshot;
  rootRequestHint: {
    kind: string;
    workspacePath: string | null;
    documentRoot: string | null;
    hydratePaths: string[];
  } | null;
  requestHint: {
    kind: string;
    workspacePath: string | null;
    documentRoot: string | null;
    hydratePaths: string[];
  } | null;
  fileCount: number;
  hydratedFileCount: number;
  hydratedPaths: string[];
  host: PreviewHostSummary;
  run: PreviewRunPlan;
  hostFiles: PreviewHostFileSummary;
};

export type PreviewBootstrapPayload = {
  preview: PreviewReadyEvent;
  workspace: SessionSnapshot;
  files: PreviewWorkspaceFile[];
  selectedFile: PreviewSelectedFile | null;
  diagnostics: PreviewDiagnostics;
};

export type UiToWorkerMessage =
  | {
      type: "worker.ping";
      requestId: string;
    }
  | {
      type: "session.create";
      requestId: string;
      sessionId?: SessionId;
      fileName: string;
      zip: ArrayBuffer;
    }
  | {
      type: "session.mount";
      requestId: string;
      session: SessionSnapshot;
      files: Array<{
        path: string;
        size: number;
        contentType: string;
        isText: boolean;
        bytes: Uint8Array;
        textContent: string | null;
      }>;
    }
  | {
      type: "session.run";
      requestId: string;
      sessionId: SessionId;
      request: RunRequest;
    }
  | {
      type: "session.stop";
      requestId: string;
      sessionId: SessionId;
    }
  | {
      type: "preview.http";
      requestId: string;
      request: VirtualHttpRequest;
    };

export type SessionCreatedEvent = {
  type: "session.created";
  requestId?: string;
  session: SessionSnapshot;
};

export type SessionStateEvent = {
  type: "session.state";
  sessionId: SessionId;
  state: SessionState;
};

export type ProcessStdoutEvent = {
  type: "process.stdout";
  sessionId: SessionId;
  pid: ProcessId;
  chunk: string;
};

export type ProcessStderrEvent = {
  type: "process.stderr";
  sessionId: SessionId;
  pid: ProcessId;
  chunk: string;
};

export type ProcessExitEvent = {
  type: "process.exit";
  sessionId: SessionId;
  pid: ProcessId;
  code: number;
};

export type RuntimeProgressStage =
  | "session-create"
  | "session-mount"
  | "host-register"
  | "runtime-launch"
  | "run-plan"
  | "runtime-context"
  | "engine-context"
  | "preview-attach"
  | "session-stop";

export type RuntimeProgressValue = string | number | boolean | null;

export type RuntimeProgressEvent = {
  type: "runtime.progress";
  sessionId: SessionId;
  stage: RuntimeProgressStage;
  message: string;
  values?: Record<string, RuntimeProgressValue>;
};

export type PreviewReadyEvent = {
  type: "preview.ready";
  sessionId: SessionId;
  pid: ProcessId;
  port: number;
  url: string;
  model: PreviewModel;
  host: PreviewHostSummary;
  run: PreviewRunPlan;
  hostFiles: PreviewHostFileSummary;
};

export type RuntimeErrorEvent = {
  type: "runtime.error";
  requestId?: string;
  sessionId: SessionId;
  error: RuntimeError;
};

export type AckMessage = {
  type: "ack";
  requestId: string;
};

export type PreviewHttpResponseMessage = {
  type: "preview.http.response";
  requestId: string;
  response: VirtualHttpResponse;
};

export type WorkerToUiMessage =
  | AckMessage
  | PreviewHttpResponseMessage
  | SessionCreatedEvent
  | SessionStateEvent
  | RuntimeProgressEvent
  | ProcessStdoutEvent
  | ProcessStderrEvent
  | ProcessExitEvent
  | PreviewReadyEvent
  | RuntimeErrorEvent;

export type RuntimeEvent = Exclude<WorkerToUiMessage, AckMessage | PreviewHttpResponseMessage>;
