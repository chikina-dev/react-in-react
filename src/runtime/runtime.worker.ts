import { guessContentType, mountArchive, type WorkspaceFileRecord } from "./analyze-archive";
import {
  createRuntimeHostAdapter,
  type HostRuntimeContext,
  type HostPreviewRequestHint,
  type HostPreviewResponseDescriptor,
  type HostRunPlan,
  type HostWorkspaceEntrySummary,
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
import {
  applyPackageJsonTextToSessionSnapshot,
  applyWorkspaceEntryToSessionSnapshot,
} from "./runtime-session-state";

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
      runtimeContext: null,
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
  const bootSummary = await hostAdapter.bootSummary();
  let runPlan: HostRunPlan;
  let runtimeContext: HostRuntimeContext;

  try {
    runPlan = await hostAdapter.planRun(sessionId, {
      cwd: request.cwd,
      command: request.command,
      args: request.args,
    });
    runtimeContext = await hostAdapter.createRuntimeContext(sessionId, {
      cwd: request.cwd,
      command: request.command,
      args: request.args,
    });
    record.runtimeContext = runtimeContext;
  } catch (error) {
    record.session.state = "errored";
    emitState(sessionId, "errored");
    emitError({
      sessionId,
      error: mapRunPlanError(error),
    });
    return;
  }

  let processInfo = runtimeContext.process;
  let processCwd = runtimeContext.process.cwd;
  let engineContextSummary = "state=<unknown> pending-jobs=<unknown>";
  let runtimeBindingsSummary = "globals=<unknown> builtins=<unknown>";
  let runtimeBootstrapSummary = "bootstrap=<unknown> modules=<unknown>";

  try {
    const engineContextResponse = await hostAdapter.describeEngineContext(runtimeContext.contextId);
    const runtimeBindingsResponse = await hostAdapter.executeRuntimeCommand(
      runtimeContext.contextId,
      {
        kind: "runtime.describe",
      },
    );
    const runtimeBootstrapResponse = await hostAdapter.executeRuntimeCommand(
      runtimeContext.contextId,
      {
        kind: "runtime.describe-bootstrap",
      },
    );
    const processInfoResponse = await hostAdapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "process.info",
    });
    const cwdResponse = await hostAdapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "process.cwd",
    });
    processInfo =
      processInfoResponse.kind === "process-info"
        ? processInfoResponse.process
        : runtimeContext.process;
    processCwd = cwdResponse.kind === "process-cwd" ? cwdResponse.cwd : processInfo.cwd;
    engineContextSummary = `state=${engineContextResponse.state} pending-jobs=${engineContextResponse.pendingJobs} entry=${engineContextResponse.entrypoint}`;
    runtimeBindingsSummary =
      runtimeBindingsResponse.kind === "runtime-bindings"
        ? `globals=${runtimeBindingsResponse.bindings.globals.join(",")} builtins=${runtimeBindingsResponse.bindings.builtins.map((builtin) => builtin.name).join(",")}`
        : runtimeBindingsSummary;
    runtimeBootstrapSummary =
      runtimeBootstrapResponse.kind === "runtime-bootstrap"
        ? `bootstrap=${runtimeBootstrapResponse.plan.bootstrapSpecifier} modules=${runtimeBootstrapResponse.plan.modules.map((module) => module.specifier).join(",")}`
        : runtimeBootstrapSummary;
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

  await emitStdout(
    sessionId,
    pid,
    `[mount] ${record.session.archive.fileCount} files available at /workspace`,
  );

  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[exec] ${request.command} ${request.args.join(" ")}`.trim(),
  );

  if (runPlan.resolvedScript) {
    await enqueueRuntimeStdout(
      hostAdapter,
      runtimeContext.contextId,
      `[script] ${runPlan.resolvedScript}`,
    );
  }

  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[host] engine=${bootSummary.engineName} interrupts=${bootSummary.supportsInterrupts} module-loader=${bootSummary.supportsModuleLoader}`,
  );
  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[host-vfs] files=${record.hostFiles.count} sample=${record.hostFiles.samplePath ?? "<none>"} size=${record.hostFiles.sampleSize ?? 0}`,
  );
  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[plan] cwd=${runPlan.cwd} entry=${runPlan.entrypoint} env=${runPlan.envCount}`,
  );
  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[process] exec=${processInfo.execPath} cwd=${processCwd} argv=${processInfo.argv.join(" ")}`,
  );
  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[engine-context] ${engineContextSummary}`,
  );
  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[bindings] ${runtimeBindingsSummary}`,
  );
  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[bootstrap] ${runtimeBootstrapSummary}`,
  );
  await enqueueRuntimeStdout(
    hostAdapter,
    runtimeContext.contextId,
    `[context] id=${runtimeContext.contextId}`,
  );
  await hostAdapter.executeRuntimeCommand(runtimeContext.contextId, {
    kind: "console.emit",
    level: "info",
    values: [
      `[detect] react=${record.session.capabilities.detectedReact} vite=${record.session.capabilities.detectedVite}`,
    ],
  });

  try {
    const bootResponse = await hostAdapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.boot-engine",
    });

    if (bootResponse.kind === "runtime-engine-boot") {
      await enqueueRuntimeStdout(
        hostAdapter,
        runtimeContext.contextId,
        `[engine-boot] pending-jobs=${bootResponse.report.pendingJobs} drained=${bootResponse.report.drainedJobs} entry=${bootResponse.report.loaderPlan.entryModule.resolvedSpecifier} loader-roots=${bootResponse.report.loaderPlan.nodeModuleSearchRoots.join(",") || "<none>"} result=${bootResponse.report.resultSummary || "<none>"}`,
      );
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : "unknown engine boot failure";
    await enqueueRuntimeStdout(
      hostAdapter,
      runtimeContext.contextId,
      `[engine-boot] bootstrap failed, synthetic fallback active: ${message}`,
    );
  }

  await flushRuntimeEvents(hostAdapter, sessionId, pid, runtimeContext.contextId);

  if (activeProcess.cancelled) {
    return;
  }

  const portResponse = await hostAdapter.executeRuntimeCommand(runtimeContext.contextId, {
    kind: "http.serve-preview",
  });
  const port = portResponse.kind === "http-server-listening" ? portResponse.server.port.port : 3000;
  await flushRuntimeEvents(hostAdapter, sessionId, pid, runtimeContext.contextId);
  const url = `/preview/${sessionId}/${port}/`;
  const model = buildPreviewModel(record.session, runPlan);
  const rootRequestHintResponse = await hostAdapter.executeRuntimeCommand(
    runtimeContext.contextId,
    {
      kind: "http.resolve-preview",
      request: {
        port,
        method: "GET",
        relativePath: "/",
        search: "",
      },
    },
  );

  record.preview = {
    pid,
    port,
    url,
    model,
    rootRequestHint:
      rootRequestHintResponse.kind === "preview-request-resolved"
        ? rootRequestHintResponse.requestHint
        : {
            kind: "fallback-root",
            workspacePath: null,
            documentRoot: null,
            hydratePaths: [],
          },
    rootResponseDescriptor:
      rootRequestHintResponse.kind === "preview-request-resolved"
        ? rootRequestHintResponse.responseDescriptor
        : {
            kind: "host-managed-fallback",
            workspacePath: null,
            documentRoot: null,
            hydratePaths: [],
            statusCode: 200,
            contentType: "text/html; charset=utf-8",
            allowMethods: [],
            omitBody: false,
          },
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
      await hostAdapter.dropRuntimeContext(contextId).catch(() => undefined);
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
    const serversResponse = await hostAdapter
      .executeRuntimeCommand(contextId, {
        kind: "http.list-servers",
      })
      .catch(() => null);
    if (serversResponse?.kind === "http-server-list") {
      for (const server of serversResponse.servers) {
        await hostAdapter
          .executeRuntimeCommand(contextId, {
            kind: "http.close-server",
            port: server.port.port,
          })
          .catch(() => undefined);
      }
    } else {
      const portsResponse = await hostAdapter
        .executeRuntimeCommand(contextId, {
          kind: "port.list",
        })
        .catch(() => null);
      if (portsResponse?.kind === "port-list") {
        for (const port of portsResponse.ports) {
          await hostAdapter
            .executeRuntimeCommand(contextId, {
              kind: "port.close",
              port: port.port,
            })
            .catch(() => undefined);
        }
      }
    }
    await flushRuntimeEvents(hostAdapter, sessionId, process.pid, contextId).catch(() => undefined);
    await hostAdapter
      .executeRuntimeCommand(contextId, {
        kind: "process.exit",
        code: 130,
      })
      .catch(() => undefined);
    await flushRuntimeEvents(hostAdapter, sessionId, process.pid, contextId).catch(() => undefined);
    await hostAdapter.dropRuntimeContext(contextId).catch(() => undefined);
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

async function enqueueRuntimeStdout(
  hostAdapter: Awaited<typeof hostAdapterPromise>,
  contextId: string,
  chunk: string,
): Promise<void> {
  await hostAdapter.executeRuntimeCommand(contextId, {
    kind: "stdio.write",
    stream: "stdout",
    chunk,
  });
}

async function flushRuntimeEvents(
  hostAdapter: Awaited<typeof hostAdapterPromise>,
  sessionId: string,
  pid: number,
  contextId: string,
): Promise<void> {
  const drained = await hostAdapter.executeRuntimeCommand(contextId, {
    kind: "runtime.drain-events",
  });

  if (drained.kind !== "runtime-events") {
    return;
  }

  for (const event of drained.events) {
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
          record.session.revision = event.revision;
          applyWorkspaceEntryChange(record, event.entry);
          await syncSessionSnapshotFromWorkspaceChange(record, hostAdapter, sessionId, event.entry);
          const refreshed = await refreshPreviewRootPlan(record, hostAdapter, contextId);
          if (refreshed) {
            await emitStdout(
              sessionId,
              pid,
              `[preview] root-plan ${record.preview?.rootResponseDescriptor.kind ?? "unknown"}`,
            );
          }
        }
        await emitStdout(sessionId, pid, `[vfs] ${event.entry.kind} ${event.entry.path} updated`);
        break;
      }
    }
  }
}

function applyWorkspaceEntryChange(record: SessionRecord, entry: HostWorkspaceEntrySummary): void {
  applyWorkspaceEntryToSessionSnapshot(record.session, entry);

  if (entry.kind === "file") {
    const nextIndex = record.hostFiles.index.filter((file) => file.path !== entry.path);
    nextIndex.push({
      path: entry.path,
      size: entry.size,
      isText: entry.isText,
    });
    nextIndex.sort((left, right) => left.path.localeCompare(right.path));
    record.hostFiles.index = nextIndex;
    record.hostFiles.count = nextIndex.length;
  }

  record.hostFileCache.delete(entry.path);

  const sample = record.hostFiles.index[0] ?? null;
  record.hostFiles.samplePath = sample?.path ?? null;
  record.hostFiles.sampleSize = sample?.size ?? null;

  if (record.preview) {
    record.preview.hostFiles = {
      count: record.hostFiles.count,
      samplePath: record.hostFiles.samplePath,
      sampleSize: record.hostFiles.sampleSize,
    };
  }
}

async function syncSessionSnapshotFromWorkspaceChange(
  record: SessionRecord,
  hostAdapter: Awaited<typeof hostAdapterPromise>,
  sessionId: string,
  entry: HostWorkspaceEntrySummary,
): Promise<void> {
  if (entry.path === "/workspace/package.json" && entry.kind === "file") {
    const packageJsonFile = await hostAdapter
      .readWorkspaceFile(sessionId, entry.path)
      .catch(() => null);
    applyPackageJsonTextToSessionSnapshot(record.session, packageJsonFile?.textContent ?? null);
  }

  if (record.preview) {
    record.preview.model = buildPreviewModel(record.session, {
      cwd: record.preview.run.cwd,
      entrypoint: record.preview.run.entrypoint,
      commandLine: record.preview.run.commandLine,
      envCount: record.preview.run.envCount,
      commandKind: record.preview.run.commandKind,
      resolvedScript: record.preview.run.resolvedScript,
    });
  }
}

async function refreshPreviewRootPlan(
  record: SessionRecord,
  hostAdapter: Awaited<typeof hostAdapterPromise>,
  contextId: string,
): Promise<boolean> {
  if (!record.preview) {
    return false;
  }

  const previousRequestHint = JSON.stringify(record.preview.rootRequestHint);
  const previousResponseDescriptor = JSON.stringify(record.preview.rootResponseDescriptor);
  const refreshed = await hostAdapter
    .executeRuntimeCommand(contextId, {
      kind: "http.resolve-preview",
      request: {
        port: record.preview.port,
        method: "GET",
        relativePath: "/",
        search: "",
      },
    })
    .catch(() => null);

  if (refreshed?.kind !== "preview-request-resolved") {
    return false;
  }

  record.preview.rootRequestHint = refreshed.requestHint;
  record.preview.rootResponseDescriptor = refreshed.responseDescriptor;

  return (
    previousRequestHint !== JSON.stringify(refreshed.requestHint) ||
    previousResponseDescriptor !== JSON.stringify(refreshed.responseDescriptor)
  );
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
    const requestHintResponse =
      record?.preview && record.runtimeContext
        ? await hostAdapter
            .executeRuntimeCommand(record.runtimeContext.contextId, {
              kind: "http.resolve-preview",
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
      requestHintResponse?.kind === "preview-request-resolved"
        ? requestHintResponse.requestHint
        : null;
    const responseDescriptor =
      requestHintResponse?.kind === "preview-request-resolved"
        ? requestHintResponse.responseDescriptor
        : null;
    const files =
      record && responseDescriptor
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
