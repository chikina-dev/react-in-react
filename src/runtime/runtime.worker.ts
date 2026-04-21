import { guessContentType, mountArchive, type WorkspaceFileRecord } from "./analyze-archive";
import {
  createRuntimeHostAdapter,
  type HostRuntimeHttpServer,
  type HostRuntimePort,
  type HostRuntimeEvent,
  type HostRuntimeStartupReport,
  type HostRuntimeContext,
  type HostPreviewRequestHint,
  type HostPreviewResponseDescriptor,
  type HostRunPlan,
  type HostRuntimeStateReport,
} from "./host-adapter";
import { buildPreviewResponse, isPreviewPath } from "./preview-server";
import type {
  PreviewHostFileSummary,
  PreviewHostSummary,
  PreviewModel,
  PreviewRunPlan,
  ProcessId,
  RuntimeError,
  SessionId,
  SessionSnapshot,
  UiToWorkerMessage,
  VirtualHttpResponse,
  WorkerToUiMessage,
} from "./protocol";

type ActiveProcess = {
  pid: ProcessId;
  cancelled: boolean;
  exitCode: number | null;
};

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
};

const hostAdapterPromise = createRuntimeHostAdapter();
const sessions = new Map<SessionId, SessionRecord>();
let nextPid = 1000;

self.addEventListener("message", (event: MessageEvent<UiToWorkerMessage>) => {
  void handleMessage(event.data);
});

async function handleMessage(message: UiToWorkerMessage): Promise<void> {
  switch (message.type) {
    case "session.create":
      await createSession(message);
      break;
    case "session.run":
      emitAck(message.requestId);
      await runSession(message.sessionId, message.request);
      break;
    case "session.stop":
      await stopSession(message.sessionId);
      emitAck(message.requestId);
      break;
    case "preview.http":
      void handlePreviewHttpRequest(message.requestId, message.request);
      break;
  }
}

async function createSession(
  message: Extract<UiToWorkerMessage, { type: "session.create" }>,
): Promise<void> {
  const sessionId = crypto.randomUUID();

  try {
    const mounted = mountArchive(message.fileName, message.zip, sessionId);
    const hostAdapter = await hostAdapterPromise;
    const handle = await hostAdapter.createSession({
      sessionId,
      session: mounted.snapshot,
      files: mounted.files,
    });
    sessions.set(sessionId, {
      session: mounted.snapshot,
      process: null,
      runtimeContext: null,
      hostFiles: {
        count: handle.fileIndex.length,
        index: handle.fileIndex,
        samplePath: handle.samplePath,
        sampleSize: handle.sampleSize,
      },
      preview: null,
      hostFileCache: new Map(),
    });
    postMessage({
      type: "session.created",
      requestId: message.requestId,
      session: mounted.snapshot,
    } satisfies WorkerToUiMessage);
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

  const hostAdapter = await hostAdapterPromise;
  let bootSummary;
  let runPlan: HostRunPlan;
  let runtimeContext: HostRuntimeContext;
  let runtimeState: HostRuntimeStateReport | null = null;
  let startupPreviewReport: {
    startup: HostRuntimeStartupReport;
    server: HostRuntimeHttpServer | null;
    port: HostRuntimePort | null;
    rootRequestHint: HostPreviewRequestHint | null;
    rootResponseDescriptor: HostPreviewResponseDescriptor | null;
  } | null = null;
  let startupLogs: string[] = [];
  let startupEvents: HostRuntimeEvent[] = [];

  try {
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
    bootSummary = runtimeLaunch.bootSummary;
    runPlan = runtimeLaunch.runPlan;
    runtimeContext = runtimeLaunch.runtimeContext;
    record.runtimeContext = runtimeContext;
    runtimeState = runtimeLaunch.state;
    startupPreviewReport = runtimeLaunch.previewLaunch;
    startupLogs = runtimeLaunch.startupLogs;
    startupEvents = runtimeLaunch.events;
    applyRuntimeStateReport(record, runtimeState);
  } catch (error) {
    record.session.state = "errored";
    emitState(sessionId, "errored");
    emitError({
      sessionId,
      error: mapRunPlanError(error),
    });
    return;
  }

  try {
    await emitStdout(
      sessionId,
      pid,
      `[mount] ${record.session.archive.fileCount} files available at /workspace`,
    );
    await emitStdout(sessionId, pid, `[exec] ${request.command} ${request.args.join(" ")}`.trim());

    if (runPlan.resolvedScript) {
      await emitStdout(sessionId, pid, `[script] ${runPlan.resolvedScript}`);
    }

    await emitStdout(
      sessionId,
      pid,
      `[host-vfs] files=${record.hostFiles.count} sample=${record.hostFiles.samplePath ?? "<none>"} size=${record.hostFiles.sampleSize ?? 0}`,
    );
    for (const line of startupLogs) {
      await emitStdout(sessionId, pid, line);
    }
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

  await emitRuntimeEvents(sessionId, pid, startupEvents);

  if (activeProcess.cancelled) {
    return;
  }

  const exitedDuringStartup = startupPreviewReport?.startup.exited ?? false;
  if (exitedDuringStartup) {
    const exitCode = startupPreviewReport?.startup.exitCode ?? activeProcess.exitCode ?? 0;
    await emitStdout(sessionId, pid, `[process] exited before preview code=${exitCode}`);
    await finalizeExitedRun(sessionId, exitCode, runtimeContext.contextId);
    return;
  }

  const previewState = runtimeState?.preview ?? null;
  const port = previewState?.port.port ?? startupPreviewReport?.port?.port ?? 3000;
  const url = `/preview/${sessionId}/${port}/`;
  const model = buildPreviewModel(record.session, previewState?.run ?? runPlan);

  record.preview = {
    pid,
    port,
    url,
    model,
    rootRequestHint: previewState?.rootRequestHint ??
      startupPreviewReport?.rootRequestHint ?? {
        kind: "fallback-root",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      },
    rootResponseDescriptor: previewState?.rootResponseDescriptor ??
      startupPreviewReport?.rootResponseDescriptor ?? {
        kind: "host-managed-fallback",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
        statusCode: 200,
        contentType: "text/html; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      },
    host: previewState?.host ?? bootSummary,
    run: previewState?.run ?? {
      cwd: runPlan.cwd,
      entrypoint: runPlan.entrypoint,
      commandLine: runPlan.commandLine,
      envCount: runPlan.envCount,
      commandKind: runPlan.commandKind,
      resolvedScript: runPlan.resolvedScript,
    },
    hostFiles: previewState?.hostFiles ?? {
      count: record.hostFiles.count,
      samplePath: record.hostFiles.samplePath,
      sampleSize: record.hostFiles.sampleSize,
    },
  };

  postMessage({
    type: "preview.ready",
    sessionId,
    pid,
    port,
    url,
    model,
    host: record.preview.host,
    run: record.preview.run,
    hostFiles: record.preview.hostFiles,
  } satisfies WorkerToUiMessage);

  await emitStdout(sessionId, pid, `[preview] server-ready ${url}`);
}

async function stopSession(sessionId: string): Promise<void> {
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
      const hostAdapter = await hostAdapterPromise;
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
    const hostAdapter = await hostAdapterPromise;
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
    postMessage({
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
      postMessage({
        type: "process.exit",
        sessionId,
        pid: process.pid,
        code,
      } satisfies WorkerToUiMessage);
    }
  }

  if (contextId) {
    const hostAdapter = await hostAdapterPromise;
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

function buildPreviewModel(session: SessionSnapshot, runPlan: HostRunPlan): PreviewModel {
  const packageName = session.packageJson?.name ?? session.archive.fileName;

  return {
    title: `${packageName} guest app`,
    summary:
      "Host React から iframe 内 DOM に別 root を生やして描画しています。次の段階でこの生成責務を Service Worker + WASM host へ寄せます。",
    cwd: runPlan.cwd,
    command: runPlan.commandLine,
    highlights: [
      `session=${session.sessionId}`,
      `revision=${session.revision}`,
      `files=${session.archive.fileCount}`,
      `run-kind=${runPlan.commandKind}`,
      runPlan.resolvedScript
        ? `resolved-script=${runPlan.resolvedScript}`
        : "resolved-script=<direct>",
      `react-detected=${String(session.capabilities.detectedReact)}`,
    ],
  };
}

async function emitStdout(sessionId: string, pid: number, chunk: string): Promise<void> {
  await sleep(180);
  postMessage({
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
        postMessage({
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
        postMessage({
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
    url: `/preview/${record.session.sessionId}/${previewPort}/`,
    model: buildPreviewModel(record.session, report.preview.run),
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
  postMessage({
    type: "session.state",
    sessionId,
    state,
  } satisfies WorkerToUiMessage);
}

function emitAck(requestId: string): void {
  postMessage({
    type: "ack",
    requestId,
  } satisfies WorkerToUiMessage);
}

function emitError(payload: { requestId?: string; sessionId: string; error: RuntimeError }): void {
  postMessage({
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

  postMessage({
    type: "preview.http.response",
    requestId,
    response,
  } satisfies WorkerToUiMessage);
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
  if (isPreviewPath(request.pathname)) {
    const hostAdapter = await hostAdapterPromise;
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
    const files =
      record && previewRequestResponse?.kind === "runtime-preview-request"
        ? await ensurePreviewFiles(
            record,
            previewRequestResponse.report.hydrationPaths,
            hydratedFiles,
          )
        : record && responseDescriptor
          ? await ensurePreviewFiles(record, responseDescriptor.hydratePaths)
          : record && requestHint
            ? await ensurePreviewFiles(record, requestHint.hydratePaths)
            : null;

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

async function ensurePreviewFiles(
  record: SessionRecord,
  hydrationPaths: string[],
  prehydratedFiles: Array<{
    path: string;
    size: number;
    isText: boolean;
    textContent: string | null;
    bytes: Uint8Array;
  }> = [],
): Promise<Map<string, WorkspaceFileRecord>> {
  const files = new Map(
    record.hostFiles.index.map((summary) => [summary.path, createPreviewFileStub(summary)]),
  );

  applyHydratedPreviewFiles(record, prehydratedFiles);

  for (const [path, file] of record.hostFileCache.entries()) {
    files.set(path, file);
  }

  const hostAdapter = await hostAdapterPromise;
  const nextPaths = hydrationPaths.filter(
    (path) => !record.hostFileCache.has(path) && files.has(path),
  );
  const hydratedFiles = await hostAdapter.readWorkspaceFiles(record.session.sessionId, nextPaths);
  applyHydratedPreviewFiles(record, hydratedFiles);

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
  const basePath = `/preview/${request.sessionId}/${request.port}`;
  const suffix = request.pathname.startsWith(basePath)
    ? request.pathname.slice(basePath.length)
    : "";
  return suffix || "/";
}
