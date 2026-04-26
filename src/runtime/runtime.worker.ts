import { guessContentType, mountArchive, type WorkspaceFileRecord } from "./analyze-archive";
import { isAppPreviewPath, stripAppBasePath, withAppBasePath } from "./app-base";
import previewReactDomClientUrl from "../preview/shims/react-dom-client.js?url";
import previewReactUrl from "../preview/shims/react.js?url";
import type {
  HostRuntimeEvent,
  HostRuntimeContext,
  HostRuntimePreviewClientModule,
  HostPreviewRequestHint,
  HostPreviewResponseDescriptor,
  HostRuntimeStateReport,
  HostWorkspaceFileContent,
  RuntimeHostAdapter,
} from "./host-adapter";
import { PREVIEW_CLIENT_HEADER } from "./preview-constants";
import type {
  PreviewHostFileSummary,
  PreviewHostSummary,
  PreviewModel,
  PreviewRunPlan,
  ProcessId,
  RuntimeProgressStage,
  RuntimeProgressValue,
  RuntimeError,
  SessionId,
  SessionSnapshot,
  UiToWorkerMessage,
  VirtualHttpResponse,
  WorkerToUiMessage,
} from "./protocol";

type PreviewBridgeConnectMessage = {
  type: "preview.bridge.connect";
};

type ActiveProcess = {
  pid: ProcessId;
  cancelled: boolean;
  exitCode: number | null;
};

function buildPreviewClientModules(
  clientScriptUrl: string | null,
): HostRuntimePreviewClientModule[] {
  const modules: HostRuntimePreviewClientModule[] = [
    {
      specifier: "react",
      url: previewReactUrl,
    },
    {
      specifier: "react-dom/client",
      url: previewReactDomClientUrl,
    },
  ];

  if (clientScriptUrl) {
    modules.unshift({
      specifier: "runtime:preview-client",
      url: clientScriptUrl,
    });
  }

  return modules;
}

type SessionRecord = {
  session: SessionSnapshot;
  process: ActiveProcess | null;
  runtimeContext: HostRuntimeContext | null;
  hostFiles: {
    count: number;
    index: Array<{
      path: string;
      size: number;
      isText: boolean;
    }>;
    samplePath: string | null;
    sampleSize: number | null;
  };
  preview: {
    pid: number;
    port: number;
    url: string;
    model: PreviewModel;
    rootRequestHint: HostPreviewRequestHint;
    rootResponseDescriptor: HostPreviewResponseDescriptor;
    host: PreviewHostSummary;
    run: PreviewRunPlan;
    hostFiles: PreviewHostFileSummary;
  } | null;
  hostFileCache: Map<string, WorkspaceFileRecord>;
  hostSessionRegistered: boolean;
};

let hostAdapterPromise: Promise<RuntimeHostAdapter> | null = null;
let previewServerModulePromise: Promise<typeof import("./preview-server")> | null = null;
const sessions = new Map<SessionId, SessionRecord>();
let nextPid = 1000;
let messageQueue = Promise.resolve();
let previewBridgePort: MessagePort | null = null;
function getHostAdapter(): Promise<RuntimeHostAdapter> {
  hostAdapterPromise ??= import("./host-adapter").then(({ createRuntimeHostAdapter }) =>
    createRuntimeHostAdapter(),
  );
  return hostAdapterPromise;
}

function getPreviewServerModule(): Promise<typeof import("./preview-server")> {
  previewServerModulePromise ??= import("./preview-server");
  return previewServerModulePromise;
}

self.addEventListener(
  "message",
  (event: MessageEvent<UiToWorkerMessage | PreviewBridgeConnectMessage>) => {
    if (event.data.type === "preview.bridge.connect") {
      connectPreviewBridge(event.ports[0]);
      return;
    }

    if (event.data.type === "preview.http") {
      void handleMessage(event.data);
      return;
    }

    messageQueue = messageQueue
      .then(async () => {
        await handleMessage(event.data as UiToWorkerMessage);
      })
      .catch((error) => {
        const message = error instanceof Error ? error.message : String(error);
        const sessionId =
          "sessionId" in event.data && typeof event.data.sessionId === "string"
            ? event.data.sessionId
            : ([...sessions.keys()].at(-1) ?? "message-queue");
        emitError({
          sessionId,
          error: {
            code: "WORKER_MESSAGE_QUEUE_FAILED",
            message,
          },
        });
      });
  },
);

self.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  const message = reason instanceof Error ? reason.message : String(reason);
  const sessionId = [...sessions.keys()].at(-1) ?? "unhandled-rejection";
  emitError({
    sessionId,
    error: {
      code: "UNHANDLED_REJECTION",
      message,
    },
  });
  event.preventDefault();
});

async function handleMessage(message: UiToWorkerMessage): Promise<void> {
  switch (message.type) {
    case "worker.ping":
      emitAck(message.requestId);
      break;
    case "session.create":
      await createSession(message);
      break;
    case "session.mount":
      await mountSession(message);
      break;
    case "session.run":
      emitAck(message.requestId);
      await runSession(message.sessionId, message.request);
      break;
    case "session.stop":
      emitAck(message.requestId);
      await stopSession(message.sessionId);
      break;
    case "preview.http":
      void handlePreviewHttpRequest(message.requestId, message.request);
      break;
  }
}

async function createSession(
  message: Extract<UiToWorkerMessage, { type: "session.create" }>,
): Promise<void> {
  const sessionId = message.sessionId ?? crypto.randomUUID();

  try {
    emitProgress(sessionId, "session-create", `Reading ${message.fileName}...`);
    const mounted = mountArchive(message.fileName, message.zip, sessionId);
    emitProgress(
      sessionId,
      "session-mount",
      `Mounting ${mounted.snapshot.archive.fileCount} files into ${mounted.snapshot.workspaceRoot}...`,
      {
        fileName: mounted.snapshot.archive.fileName,
        fileCount: mounted.snapshot.archive.fileCount,
        directoryCount: mounted.snapshot.archive.directoryCount,
        workspaceRoot: mounted.snapshot.workspaceRoot,
      },
    );
    await registerMountedSession(message.requestId, mounted.snapshot, mounted.files);
    emitAck(message.requestId);
  } catch (error) {
    emitError({
      requestId: message.requestId,
      sessionId,
      error: {
        code: "ZIP_PARSE_FAILED",
        message: error instanceof Error ? error.message : "Failed to read ZIP",
      },
    });
  }
}

async function mountSession(
  message: Extract<UiToWorkerMessage, { type: "session.mount" }>,
): Promise<void> {
  try {
    emitProgress(
      message.session.sessionId,
      "session-mount",
      `Mounting ${message.session.archive.fileCount} files into ${message.session.workspaceRoot}...`,
      {
        fileName: message.session.archive.fileName,
        fileCount: message.session.archive.fileCount,
        directoryCount: message.session.archive.directoryCount,
        workspaceRoot: message.session.workspaceRoot,
      },
    );
    const files = new Map(
      message.files.map((file) => [
        file.path,
        {
          ...file,
          bytes: new Uint8Array(file.bytes),
        },
      ]),
    );
    await registerMountedSession(message.requestId, message.session, files);
    emitAck(message.requestId);
  } catch (error) {
    emitError({
      requestId: message.requestId,
      sessionId: message.session.sessionId,
      error: {
        code: "SESSION_MOUNT_FAILED",
        message: error instanceof Error ? error.message : "Failed to mount session",
      },
    });
  }
}

async function registerMountedSession(
  requestId: string,
  session: SessionSnapshot,
  files: Map<string, WorkspaceFileRecord>,
): Promise<void> {
  const hostFiles = buildHostFileSummary(files);
  sessions.set(session.sessionId, {
    session,
    process: null,
    runtimeContext: null,
    hostFiles,
    preview: null,
    hostFileCache: new Map(
      [...files.entries()].map(([path, file]) => [path, { ...file, bytes: file.bytes.slice() }]),
    ),
    hostSessionRegistered: false,
  });

  self.postMessage({
    type: "session.created",
    requestId,
    session,
  } satisfies WorkerToUiMessage);
}

function buildHostFileSummary(files: Map<string, WorkspaceFileRecord>): SessionRecord["hostFiles"] {
  const index = [...files.values()]
    .map((file) => ({
      path: file.path,
      size: file.size,
      isText: file.isText,
    }))
    .sort((left, right) => left.path.localeCompare(right.path));

  return {
    count: index.length,
    index,
    samplePath: index[0]?.path ?? null,
    sampleSize: index[0]?.size ?? null,
  };
}

async function ensureHostSessionRegistered(record: SessionRecord): Promise<void> {
  if (record.hostSessionRegistered) {
    return;
  }

  const hostAdapter = await getHostAdapter();
  const files = new Map(
    record.hostFiles.index.map((summary) => {
      const cached = record.hostFileCache.get(summary.path);
      if (!cached) {
        throw new Error(`Missing workspace file cache entry for ${summary.path}`);
      }
      return [summary.path, cached] as const;
    }),
  );

  const handle = await hostAdapter.createSession({
    sessionId: record.session.sessionId,
    session: record.session,
    files,
  });

  record.hostFiles = {
    count: handle.fileIndex.length,
    index: handle.fileIndex,
    samplePath: handle.samplePath,
    sampleSize: handle.sampleSize,
  };
  record.hostSessionRegistered = true;
}

async function runSession(
  sessionId: string,
  request: { cwd: string; command: string; args: string[] },
): Promise<void> {
  const record = sessions.get(sessionId);

  if (!record) {
    emitError({
      sessionId,
      error: {
        code: "SESSION_NOT_FOUND",
        message: "Session was not found.",
      },
    });
    return;
  }

  await disposeActiveRun(sessionId);

  const pid = nextPid++;
  const activeProcess: ActiveProcess = {
    pid,
    cancelled: false,
    exitCode: null,
  };

  record.process = activeProcess;
  record.session.state = "running";
  emitState(sessionId, "running");

  try {
    emitProgress(sessionId, "host-register", "Registering workspace with runtime host...", {
      hostFileCount: record.hostFiles.count,
      samplePath: record.hostFiles.samplePath,
    });
    await ensureHostSessionRegistered(record);
    const hostAdapter = await getHostAdapter();
    let runtimeContext: HostRuntimeContext;
    let runtimeState: HostRuntimeStateReport | null = null;
    let startupExited = false;
    let startupExitCode: number | null = null;
    let startupStdout: string[] = [];
    let previewReady: {
      port: number;
      url: string;
      model: PreviewModel;
      rootHydratedFiles: HostWorkspaceFileContent[];
      host: PreviewHostSummary;
      run: PreviewRunPlan;
      hostFiles: PreviewHostFileSummary;
    } | null = null;
    let startupEvents: HostRuntimeEvent[] = [];

    emitProgress(
      sessionId,
      "runtime-launch",
      `Launching ${[request.command, ...request.args].join(" ").trim()}...`,
      {
        cwd: request.cwd,
        command: request.command,
        args: request.args.join(" "),
      },
    );
    const runtimeLaunch = await hostAdapter.launchRuntime(
      sessionId,
      {
        cwd: request.cwd,
        command: request.command,
        args: request.args,
      },
      {
        maxTurns: 64,
      },
    );
    runtimeContext = runtimeLaunch.runtimeContext;
    record.runtimeContext = runtimeContext;
    runtimeState = runtimeLaunch.state;
    startupExited = runtimeLaunch.previewLaunch.startup.exited;
    startupExitCode = runtimeLaunch.previewLaunch.startup.exitCode;
    startupStdout = runtimeLaunch.startupStdout;
    previewReady =
      runtimeLaunch.previewReady == null
        ? null
        : {
            port: runtimeLaunch.previewReady.port.port,
            url: runtimeLaunch.previewReady.url,
            model: runtimeLaunch.previewReady.model,
            rootHydratedFiles: runtimeLaunch.previewReady.rootHydratedFiles,
            host: runtimeLaunch.previewReady.host,
            run: runtimeLaunch.previewReady.run,
            hostFiles: runtimeLaunch.previewReady.hostFiles,
          };
    startupEvents = runtimeLaunch.events;
    applyRuntimeStateReport(record, runtimeState);

    emitProgress(sessionId, "run-plan", "Runtime host produced a run plan.", {
      commandKind: runtimeLaunch.runPlan.commandKind,
      cwd: runtimeLaunch.runPlan.cwd,
      entrypoint: runtimeLaunch.runPlan.entrypoint,
      commandLine: runtimeLaunch.runPlan.commandLine,
      envCount: runtimeLaunch.runPlan.envCount,
    });

    emitProgress(sessionId, "runtime-context", "Runtime context is available.", {
      contextId: runtimeLaunch.runtimeContext.contextId,
      execPath: runtimeLaunch.runtimeContext.process.execPath,
      argvLen: runtimeLaunch.runtimeContext.process.argv.length,
      platform: runtimeLaunch.runtimeContext.process.platform,
    });

    emitProgress(sessionId, "engine-context", "Engine context is ready.", {
      engineContextId: runtimeLaunch.engineContext.engineContextId,
      engineSessionId: runtimeLaunch.engineContext.engineSessionId,
      state: runtimeLaunch.engineContext.state,
      bridgeReady: runtimeLaunch.engineContext.bridgeReady,
      pendingJobs: runtimeLaunch.engineContext.pendingJobs,
      registeredModules: runtimeLaunch.engineContext.registeredModules,
    });

    for (const line of startupStdout) {
      await emitStdout(sessionId, pid, line);
    }

    await emitRuntimeEvents(sessionId, pid, startupEvents);

    if (activeProcess.cancelled) {
      return;
    }

    const exitedDuringStartup = startupExited;
    if (exitedDuringStartup) {
      const exitCode = startupExitCode ?? activeProcess.exitCode ?? 0;
      await finalizeExitedRun(sessionId, exitCode, runtimeContext.contextId);
      return;
    }

    const previewState = runtimeState?.preview;
    if (!previewState || !previewReady) {
      record.session.state = "errored";
      emitState(sessionId, "errored");
      emitError({
        sessionId,
        error: {
          code: "PREVIEW_STATE_MISSING",
          message: "Backend launch report did not include preview state.",
        },
      });
      return;
    }

    applyHydratedPreviewFiles(record, previewReady.rootHydratedFiles);
    emitProgress(sessionId, "preview-attach", `Attaching preview at ${previewReady.url}...`, {
      port: previewReady.port,
      url: previewReady.url,
      engineName: previewReady.host.engineName,
      hostFileCount: previewReady.hostFiles.count,
    });

    record.preview = {
      pid,
      port: previewState.port.port,
      url: previewState.url,
      model: previewState.model,
      rootRequestHint: previewState.rootRequestHint,
      rootResponseDescriptor: previewState.rootResponseDescriptor,
      host: previewState.host,
      run: previewState.run,
      hostFiles: previewState.hostFiles,
    };

    self.postMessage({
      type: "preview.ready",
      sessionId,
      pid,
      port: previewReady.port,
      url: previewReady.url,
      model: previewReady.model,
      host: previewReady.host,
      run: previewReady.run,
      hostFiles: previewReady.hostFiles,
    } satisfies WorkerToUiMessage);
  } catch (error) {
    await disposeActiveRun(sessionId);
    record.session.state = "errored";
    emitState(sessionId, "errored");
    emitError({
      sessionId,
      error: mapRunPlanError(error),
    });
    return;
  }
}

async function stopSession(sessionId: string): Promise<void> {
  emitProgress(sessionId, "session-stop", "Stopping runtime session...");
  await disposeActiveRun(sessionId);
}

async function disposeActiveRun(sessionId: string): Promise<void> {
  const record = sessions.get(sessionId);

  if (!record) {
    return;
  }

  if (!record.process) {
    if (record.runtimeContext) {
      const contextId = record.runtimeContext.contextId;
      record.runtimeContext = null;
      const hostAdapter = await getHostAdapter();
      await hostAdapter
        .executeRuntimeCommand(contextId, {
          kind: "runtime.shutdown",
          code: 0,
        })
        .catch(() => undefined);
    }
    record.preview = null;
    record.session.state = "stopped";
    emitState(sessionId, "stopped");
    return;
  }

  const { process } = record;
  const contextId = record.runtimeContext?.contextId ?? null;
  process.cancelled = true;
  process.exitCode = 130;

  if (contextId) {
    const hostAdapter = await getHostAdapter();
    const shutdownResponse = await hostAdapter
      .executeRuntimeCommand(contextId, {
        kind: "runtime.shutdown",
        code: 130,
      })
      .catch(() => null);
    if (shutdownResponse?.kind === "runtime-shutdown") {
      await emitRuntimeEvents(sessionId, process.pid, shutdownResponse.report.events);
    }
  } else {
    self.postMessage({
      type: "process.exit",
      sessionId,
      pid: process.pid,
      code: 130,
    } satisfies WorkerToUiMessage);
  }

  record.process = null;
  record.runtimeContext = null;
  record.preview = null;
  record.session.state = "stopped";
  emitState(sessionId, "stopped");
}

async function finalizeExitedRun(
  sessionId: string,
  code: number,
  contextId: string | null,
): Promise<void> {
  const record = sessions.get(sessionId);

  if (!record) {
    return;
  }

  const process = record.process;
  if (process && process.exitCode === null) {
    process.exitCode = code;
    if (!contextId) {
      self.postMessage({
        type: "process.exit",
        sessionId,
        pid: process.pid,
        code,
      } satisfies WorkerToUiMessage);
    }
  }

  if (contextId) {
    const hostAdapter = await getHostAdapter();
    const shutdownResponse = await hostAdapter
      .executeRuntimeCommand(contextId, {
        kind: "runtime.shutdown",
        code,
      })
      .catch(() => null);
    if (shutdownResponse?.kind === "runtime-shutdown") {
      await emitRuntimeEvents(sessionId, process?.pid ?? 0, shutdownResponse.report.events);
    }
  }

  record.process = null;
  record.runtimeContext = null;
  record.preview = null;
  record.session.state = "stopped";
  emitState(sessionId, "stopped");
}

async function emitStdout(sessionId: string, pid: number, chunk: string): Promise<void> {
  await sleep(180);
  self.postMessage({
    type: "process.stdout",
    sessionId,
    pid,
    chunk,
  } satisfies WorkerToUiMessage);
}

async function emitRuntimeEvents(
  sessionId: string,
  pid: number,
  events: HostRuntimeEvent[],
): Promise<void> {
  for (const event of events) {
    switch (event.kind) {
      case "stdout":
        await emitStdout(sessionId, pid, event.chunk);
        break;
      case "stderr":
        self.postMessage({
          type: "process.stderr",
          sessionId,
          pid,
          chunk: event.chunk,
        } satisfies WorkerToUiMessage);
        break;
      case "process-exit":
        {
          const record = sessions.get(sessionId);
          if (record?.process) {
            record.process.exitCode = event.code;
          }
        }
        self.postMessage({
          type: "process.exit",
          sessionId,
          pid,
          code: event.code,
        } satisfies WorkerToUiMessage);
        break;
      case "console":
        break;
      case "port-listen":
        await emitStdout(
          sessionId,
          pid,
          `[port] ${event.port.protocol} ${event.port.port} listening`,
        );
        break;
      case "port-close":
        await emitStdout(sessionId, pid, `[port] ${event.port} closed`);
        break;
      case "workspace-change": {
        const record = sessions.get(sessionId);
        if (record) {
          applyRuntimeStateReport(record, event.state);
          await emitStdout(
            sessionId,
            pid,
            `[preview] root-plan ${record.preview?.rootResponseDescriptor.kind ?? "unknown"}`,
          );
        }
        await emitStdout(sessionId, pid, `[vfs] ${event.entry.kind} ${event.entry.path} updated`);
        break;
      }
    }
  }
}

function applyRuntimeStateReport(record: SessionRecord, report: HostRuntimeStateReport): void {
  record.session = cloneSessionSnapshot(report.session);
  record.hostFiles = cloneHostFileSummary(report.session.hostFiles);

  const knownPaths = new Set(record.hostFiles.index.map((file) => file.path));
  for (const path of record.hostFileCache.keys()) {
    if (!knownPaths.has(path)) {
      record.hostFileCache.delete(path);
    }
  }

  if (!report.preview) {
    record.preview = null;
    return;
  }

  const previewPid = record.preview?.pid ?? record.process?.pid ?? -1;
  const previewPort = report.preview.port.port;
  record.preview = {
    pid: previewPid,
    port: previewPort,
    url: withAppBasePath(report.preview.url),
    model: report.preview.model,
    rootRequestHint: report.preview.rootRequestHint,
    rootResponseDescriptor: report.preview.rootResponseDescriptor,
    host: report.preview.host,
    run: report.preview.run,
    hostFiles: {
      count: report.preview.hostFiles.count,
      samplePath: report.preview.hostFiles.samplePath,
      sampleSize: report.preview.hostFiles.sampleSize,
    },
  };
}

function cloneSessionSnapshot(report: HostRuntimeStateReport["session"]): SessionSnapshot {
  return {
    sessionId: report.sessionId,
    state: report.state,
    revision: report.revision,
    workspaceRoot: report.workspaceRoot,
    archive: {
      ...report.archive,
      entries: [...report.archive.entries],
    },
    packageJson: report.packageJson
      ? {
          ...report.packageJson,
          scripts: { ...report.packageJson.scripts },
          dependencies: [...report.packageJson.dependencies],
          devDependencies: [...report.packageJson.devDependencies],
        }
      : null,
    suggestedRunRequest: report.suggestedRunRequest
      ? {
          cwd: report.suggestedRunRequest.cwd,
          command: report.suggestedRunRequest.command,
          args: [...report.suggestedRunRequest.args],
          env: report.suggestedRunRequest.env ? { ...report.suggestedRunRequest.env } : undefined,
        }
      : null,
    capabilities: { ...report.capabilities },
  };
}

function cloneHostFileSummary(
  summary: HostRuntimeStateReport["session"]["hostFiles"],
): SessionRecord["hostFiles"] {
  return {
    count: summary.count,
    index: summary.index.map((file) => ({ ...file })),
    samplePath: summary.samplePath,
    sampleSize: summary.sampleSize,
  };
}

function emitState(sessionId: string, state: SessionSnapshot["state"]): void {
  self.postMessage({
    type: "session.state",
    sessionId,
    state,
  } satisfies WorkerToUiMessage);
}

function emitProgress(
  sessionId: string,
  stage: RuntimeProgressStage,
  message: string,
  values?: Record<string, RuntimeProgressValue>,
): void {
  self.postMessage({
    type: "runtime.progress",
    sessionId,
    stage,
    message,
    values,
  } satisfies WorkerToUiMessage);
}

function emitAck(requestId: string): void {
  self.postMessage({
    type: "ack",
    requestId,
  } satisfies WorkerToUiMessage);
}

function emitError(payload: { requestId?: string; sessionId: string; error: RuntimeError }): void {
  self.postMessage({
    type: "runtime.error",
    requestId: payload.requestId,
    sessionId: payload.sessionId,
    error: payload.error,
  } satisfies WorkerToUiMessage);
}

async function handlePreviewHttpRequest(
  requestId: string,
  request: Extract<UiToWorkerMessage, { type: "preview.http" }>["request"],
): Promise<void> {
  const record = sessions.get(request.sessionId);
  const response = await resolvePreviewHttpResponse(request, record ?? null);

  self.postMessage({
    type: "preview.http.response",
    requestId,
    response,
  } satisfies WorkerToUiMessage);
}

function connectPreviewBridge(port: MessagePort | null | undefined): void {
  if (!port) {
    return;
  }

  if (previewBridgePort) {
    previewBridgePort.onmessage = null;
  }

  previewBridgePort = port;
  previewBridgePort.onmessage = (
    event: MessageEvent<{
      type?: string;
      requestId?: string;
      request?: Extract<UiToWorkerMessage, { type: "preview.http" }>["request"];
    }>,
  ) => {
    const message = event.data;
    if (
      !message ||
      message.type !== "preview.http.request" ||
      typeof message.requestId !== "string" ||
      !message.request
    ) {
      return;
    }

    void handlePreviewHttpRequestViaPort(port, message.requestId, message.request);
  };
  if (typeof previewBridgePort.start === "function") {
    previewBridgePort.start();
  }
}

async function handlePreviewHttpRequestViaPort(
  port: MessagePort,
  requestId: string,
  request: Extract<UiToWorkerMessage, { type: "preview.http" }>["request"],
): Promise<void> {
  const record = sessions.get(request.sessionId);
  const response = await resolvePreviewHttpResponse(request, record ?? null);

  port.postMessage({
    type: "preview.http.response",
    requestId,
    response,
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function mapRunPlanError(error: unknown): RuntimeError {
  const message = error instanceof Error ? error.message : "Failed to resolve run plan.";

  if (message.startsWith("script not found: ")) {
    const scriptName = message.slice("script not found: ".length);
    return {
      code: "SCRIPT_NOT_FOUND",
      message: `Script "${scriptName}" was not found in package.json.`,
    };
  }

  if (message === "node entrypoint is required") {
    return {
      code: "ENTRYPOINT_REQUIRED",
      message: "A node entrypoint is required for `node <entry>` commands.",
    };
  }

  if (message.startsWith("unsupported command: ")) {
    return {
      code: "UNSUPPORTED_COMMAND",
      message: "Only `npm run <script>` and `node <entry>` are supported in this prototype.",
      detail: message.slice("unsupported command: ".length),
    };
  }

  if (message.startsWith("working directory must stay under /workspace: ")) {
    return {
      code: "INVALID_CWD",
      message: "Working directory must stay under /workspace.",
      detail: message.slice("working directory must stay under /workspace: ".length),
    };
  }

  if (message.startsWith("entrypoint not found: ")) {
    return {
      code: "ENTRYPOINT_NOT_FOUND",
      message: "The requested node entrypoint could not be found in the mounted workspace.",
      detail: message.slice("entrypoint not found: ".length),
    };
  }

  return {
    code: "RUN_PLAN_FAILED",
    message,
  };
}

async function resolvePreviewHttpResponse(
  request: Extract<UiToWorkerMessage, { type: "preview.http" }>["request"],
  record: SessionRecord | null,
): Promise<VirtualHttpResponse> {
  if (isAppPreviewPath(request.pathname)) {
    const hostAdapter = await getHostAdapter();
    const previewRequestResponse =
      record?.preview && record.runtimeContext
        ? await hostAdapter
            .executeRuntimeCommand(record.runtimeContext.contextId, {
              kind: "runtime.preview-request",
              request: {
                port: request.port,
                method: request.method,
                relativePath: getPreviewRelativePath(request),
                search: request.search,
                clientModules: buildPreviewClientModules(
                  request.headers[PREVIEW_CLIENT_HEADER] ?? null,
                ),
              },
            })
            .catch(() => null)
        : null;
    const requestHint =
      previewRequestResponse?.kind === "runtime-preview-request"
        ? previewRequestResponse.report.requestHint
        : null;
    const responseDescriptor =
      previewRequestResponse?.kind === "runtime-preview-request"
        ? previewRequestResponse.report.responseDescriptor
        : null;
    const hydratedFiles =
      previewRequestResponse?.kind === "runtime-preview-request"
        ? previewRequestResponse.report.hydratedFiles
        : [];
    const modulePlan =
      previewRequestResponse?.kind === "runtime-preview-request"
        ? previewRequestResponse.report.modulePlan
        : null;
    const transformKind =
      previewRequestResponse?.kind === "runtime-preview-request"
        ? previewRequestResponse.report.transformKind
        : null;
    const renderPlan =
      previewRequestResponse?.kind === "runtime-preview-request"
        ? previewRequestResponse.report.renderPlan
        : null;
    const directResponse =
      previewRequestResponse?.kind === "runtime-preview-request"
        ? previewRequestResponse.report.directResponse
        : null;
    const files =
      record && previewRequestResponse?.kind === "runtime-preview-request"
        ? ensurePreviewFiles(record, hydratedFiles)
        : record && responseDescriptor
          ? ensurePreviewFiles(record)
          : record && requestHint
            ? ensurePreviewFiles(record)
            : null;

    if (directResponse) {
      return {
        status: directResponse.status,
        headers: directResponse.headers,
        body:
          directResponse.bytes ??
          rewritePreviewPublicUrlPlaceholder(
            directResponse.textBody,
            record?.preview?.url ?? null,
          ) ??
          "",
      };
    }

    const { buildPreviewResponse } = await getPreviewServerModule();

    return buildPreviewResponse(
      request,
      record?.preview
        ? {
            sessionId: record.session.sessionId,
            pid: record.preview.pid,
            port: record.preview.port,
            url: record.preview.url,
            model: record.preview.model,
            rootRequestHint: record.preview.rootRequestHint,
            rootResponseDescriptor: record.preview.rootResponseDescriptor,
            requestHint: requestHint ?? undefined,
            responseDescriptor: responseDescriptor ?? undefined,
            transformKind: transformKind ?? undefined,
            renderPlan: renderPlan ?? undefined,
            modulePlan: modulePlan ?? undefined,
            host: record.preview.host,
            run: record.preview.run,
            hostFiles: record.preview.hostFiles,
            session: record.session,
            files: files ?? new Map(),
          }
        : null,
    );
  }

  return {
    status: 404,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
    body: JSON.stringify({
      error: "Unsupported preview path",
      pathname: request.pathname,
    }),
  };
}

function ensurePreviewFiles(
  record: SessionRecord,
  prehydratedFiles: Array<{
    path: string;
    size: number;
    isText: boolean;
    textContent: string | null;
    bytes: Uint8Array;
  }> = [],
): Map<string, WorkspaceFileRecord> {
  const files = new Map(
    record.hostFiles.index.map((summary) => [summary.path, createPreviewFileStub(summary)]),
  );

  applyHydratedPreviewFiles(record, prehydratedFiles);

  for (const [path, file] of record.hostFileCache.entries()) {
    files.set(path, file);
  }

  return files;
}

function applyHydratedPreviewFiles(
  record: SessionRecord,
  hydratedFiles: Array<{
    path: string;
    size: number;
    isText: boolean;
    textContent: string | null;
    bytes: Uint8Array;
  }>,
): void {
  for (const file of hydratedFiles) {
    record.hostFileCache.set(file.path, {
      path: file.path,
      size: file.size,
      contentType: guessContentType(file.path),
      isText: file.isText,
      bytes: file.bytes,
      textContent: file.textContent,
    });
  }
}

function createPreviewFileStub(summary: {
  path: string;
  size: number;
  isText: boolean;
}): WorkspaceFileRecord {
  return {
    path: summary.path,
    size: summary.size,
    contentType: guessContentType(summary.path),
    isText: summary.isText,
    bytes: new Uint8Array(),
    textContent: null,
  };
}

function getPreviewRelativePath(
  request: Extract<UiToWorkerMessage, { type: "preview.http" }>["request"],
): string {
  const pathname = stripAppBasePath(request.pathname);
  const basePath = `/preview/${request.sessionId}/${request.port}`;
  const suffix = pathname.startsWith(basePath) ? pathname.slice(basePath.length) : "";
  return suffix || "/";
}

function rewritePreviewPublicUrlPlaceholder(
  textBody: string | null,
  previewUrl: string | null,
): string | null {
  if (textBody == null || previewUrl == null || !textBody.includes("%PUBLIC_URL%")) {
    return textBody;
  }

  return textBody.replaceAll("%PUBLIC_URL%", previewUrl.replace(/\/$/, ""));
}
