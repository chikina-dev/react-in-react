import { guessContentType, mountArchive, type WorkspaceFileRecord } from "./analyze-archive";
import {
  createRuntimeHostAdapter,
  type HostProcessInfo,
  type HostPreviewRequestHint,
  type HostRunPlan,
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
      stopSession(message.sessionId);
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
    await hostAdapter.createSession({
      sessionId,
      session: mounted.snapshot,
      files: mounted.files,
    });
    const hostFiles = await hostAdapter.listWorkspaceFiles(sessionId);
    const sampleFile = hostFiles[0]
      ? await hostAdapter.readWorkspaceFile(sessionId, hostFiles[0].path)
      : null;
    sessions.set(sessionId, {
      session: mounted.snapshot,
      process: null,
      hostFiles: {
        count: hostFiles.length,
        index: hostFiles,
        samplePath: hostFiles[0]?.path ?? null,
        sampleSize: sampleFile?.size ?? null,
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

  stopSession(sessionId);

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
  const bootSummary = await hostAdapter.bootSummary();
  let runPlan: HostRunPlan;
  let processInfo: HostProcessInfo;

  try {
    runPlan = await hostAdapter.planRun(sessionId, {
      cwd: request.cwd,
      command: request.command,
      args: request.args,
    });
    processInfo = await hostAdapter.buildProcessInfo(sessionId, {
      cwd: request.cwd,
      command: request.command,
      args: request.args,
    });
  } catch (error) {
    record.session.state = "errored";
    emitState(sessionId, "errored");
    emitError({
      sessionId,
      error: mapRunPlanError(error),
    });
    return;
  }

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
    `[host] engine=${bootSummary.engineName} interrupts=${bootSummary.supportsInterrupts} module-loader=${bootSummary.supportsModuleLoader}`,
  );
  await emitStdout(
    sessionId,
    pid,
    `[host-vfs] files=${record.hostFiles.count} sample=${record.hostFiles.samplePath ?? "<none>"} size=${record.hostFiles.sampleSize ?? 0}`,
  );
  await emitStdout(
    sessionId,
    pid,
    `[plan] cwd=${runPlan.cwd} entry=${runPlan.entrypoint} env=${runPlan.envCount}`,
  );
  await emitStdout(
    sessionId,
    pid,
    `[process] exec=${processInfo.execPath} cwd=${processInfo.cwd} argv=${processInfo.argv.join(" ")}`,
  );

  await emitStdout(
    sessionId,
    pid,
    `[detect] react=${record.session.capabilities.detectedReact} vite=${record.session.capabilities.detectedVite}`,
  );

  if (activeProcess.cancelled) {
    return;
  }

  const port = 3000;
  const url = `/preview/${sessionId}/${port}/`;
  const model = buildPreviewModel(record.session, runPlan);

  record.preview = {
    pid,
    port,
    url,
    model,
    rootRequestHint: await hostAdapter.resolvePreviewRequestHint(sessionId, "/"),
    host: bootSummary,
    run: {
      cwd: runPlan.cwd,
      entrypoint: runPlan.entrypoint,
      commandLine: runPlan.commandLine,
      envCount: runPlan.envCount,
      commandKind: runPlan.commandKind,
      resolvedScript: runPlan.resolvedScript,
    },
    hostFiles: {
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

function stopSession(sessionId: string): void {
  const record = sessions.get(sessionId);

  if (!record?.process) {
    void hostAdapterPromise.then((adapter) => adapter.stopSession(sessionId));
    return;
  }

  const { process } = record;
  process.cancelled = true;
  process.exitCode = 130;
  record.process = null;
  record.preview = null;
  record.session.state = "stopped";
  emitState(sessionId, "stopped");
  void hostAdapterPromise.then((adapter) => adapter.stopSession(sessionId));

  postMessage({
    type: "process.exit",
    sessionId,
    pid: process.pid,
    code: 130,
  } satisfies WorkerToUiMessage);
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
    const requestHint = record
      ? await hostAdapter.resolvePreviewRequestHint(
          record.session.sessionId,
          getPreviewRelativePath(request),
        )
      : null;
    const files =
      record && requestHint ? await ensurePreviewFiles(record, requestHint.hydratePaths) : null;

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
            requestHint: requestHint ?? undefined,
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
): Promise<Map<string, WorkspaceFileRecord>> {
  const files = new Map(
    record.hostFiles.index.map((summary) => [summary.path, createPreviewFileStub(summary)]),
  );

  for (const [path, file] of record.hostFileCache.entries()) {
    files.set(path, file);
  }

  const hostAdapter = await hostAdapterPromise;
  const nextPaths = hydrationPaths.filter(
    (path) => !record.hostFileCache.has(path) && files.has(path),
  );
  const hydratedFiles = await hostAdapter.readWorkspaceFiles(record.session.sessionId, nextPaths);

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

  for (const [path, file] of record.hostFileCache.entries()) {
    files.set(path, file);
  }

  return files;
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
