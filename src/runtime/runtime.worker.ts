import { guessContentType, mountArchive, type WorkspaceFileRecord } from "./analyze-archive";
import { createRuntimeHostAdapter } from "./host-adapter";
import {
  buildPreviewResponse,
  isPreviewPath,
  PREVIEW_APP_ENTRY_CANDIDATES,
  PREVIEW_DOCUMENT_CANDIDATES,
} from "./preview-server";
import type {
  PreviewModel,
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

  const resolved = resolveCommand(record.session, request.command, request.args);

  if (!resolved.ok) {
    record.session.state = "errored";
    emitState(sessionId, "errored");
    emitError({
      sessionId,
      error: resolved.error,
    });
    return;
  }

  await emitStdout(
    sessionId,
    pid,
    `[mount] ${record.session.archive.fileCount} files available at /workspace`,
  );
  await emitStdout(sessionId, pid, `[exec] ${request.command} ${request.args.join(" ")}`.trim());

  if (resolved.script) {
    await emitStdout(sessionId, pid, `[script] ${resolved.script}`);
  }

  const hostAdapter = await hostAdapterPromise;
  const bootSummary = await hostAdapter.bootSummary();
  const runPlan = await hostAdapter.planRun(sessionId, {
    cwd: request.cwd,
    command: request.command,
    args: request.args,
  });

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
    `[detect] react=${record.session.capabilities.detectedReact} vite=${record.session.capabilities.detectedVite}`,
  );

  if (activeProcess.cancelled) {
    return;
  }

  const port = 3000;
  const url = `/preview/${sessionId}/${port}/`;
  const model = buildPreviewModel(record.session, request.command, request.args, resolved.script);

  record.preview = {
    pid,
    port,
    url,
    model,
  };

  postMessage({
    type: "preview.ready",
    sessionId,
    pid,
    port,
    url,
    model,
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

function buildPreviewModel(
  session: SessionSnapshot,
  command: string,
  args: string[],
  script: string | null,
): PreviewModel {
  const packageName = session.packageJson?.name ?? session.archive.fileName;
  const commandLine = [command, ...args].join(" ");

  return {
    title: `${packageName} guest app`,
    summary:
      "Host React から iframe 内 DOM に別 root を生やして描画しています。次の段階でこの生成責務を Service Worker + WASM host へ寄せます。",
    cwd: "/workspace",
    command: commandLine,
    highlights: [
      `session=${session.sessionId}`,
      `files=${session.archive.fileCount}`,
      script ? `resolved-script=${script}` : "resolved-script=<direct>",
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

function resolveCommand(
  session: SessionSnapshot,
  command: string,
  args: string[],
): { ok: true; script: string | null } | { ok: false; error: RuntimeError } {
  if (command === "npm" && args[0] === "run" && args[1]) {
    const scriptName = args[1];
    const script = session.packageJson?.scripts[scriptName];

    if (!script) {
      return {
        ok: false,
        error: {
          code: "SCRIPT_NOT_FOUND",
          message: `Script "${scriptName}" was not found in package.json.`,
        },
      };
    }

    return {
      ok: true,
      script,
    };
  }

  if (command === "node" && args[0]) {
    return {
      ok: true,
      script: null,
    };
  }

  return {
    ok: false,
    error: {
      code: "UNSUPPORTED_COMMAND",
      message: "Only `npm run <script>` and `node <entry>` are supported in this prototype.",
    },
  };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function resolvePreviewHttpResponse(
  request: Extract<UiToWorkerMessage, { type: "preview.http" }>["request"],
  record: SessionRecord | null,
): Promise<VirtualHttpResponse> {
  if (isPreviewPath(request.pathname)) {
    const files = record ? await ensurePreviewFiles(request, record) : null;

    return buildPreviewResponse(
      request,
      record?.preview
        ? {
            sessionId: record.session.sessionId,
            pid: record.preview.pid,
            port: record.preview.port,
            url: record.preview.url,
            model: record.preview.model,
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
  request: Extract<UiToWorkerMessage, { type: "preview.http" }>["request"],
  record: SessionRecord,
): Promise<Map<string, WorkspaceFileRecord>> {
  const files = new Map(
    record.hostFiles.index.map((summary) => [summary.path, createPreviewFileStub(summary)]),
  );

  for (const [path, file] of record.hostFileCache.entries()) {
    files.set(path, file);
  }

  const hostAdapter = await hostAdapterPromise;
  const hydrationPaths = collectPreviewHydrationPaths(request, record);
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

function collectPreviewHydrationPaths(
  request: Extract<UiToWorkerMessage, { type: "preview.http" }>["request"],
  record: SessionRecord,
): string[] {
  const paths = new Set<string>();
  const relativePath = getPreviewRelativePath(request);

  for (const summary of record.hostFiles.index) {
    if (summary.path.endsWith("/package.json")) {
      paths.add(summary.path);
    }
  }

  if (relativePath === "/" || relativePath === "/index.html") {
    for (const candidate of PREVIEW_DOCUMENT_CANDIDATES) {
      paths.add(candidate);
    }

    for (const candidate of PREVIEW_APP_ENTRY_CANDIDATES) {
      paths.add(candidate);
    }

    return [...paths];
  }

  if (relativePath.startsWith("/files/")) {
    paths.add(decodeWorkspacePath(relativePath));
    return [...paths];
  }

  if (relativePath.startsWith("/__") || relativePath === "/assets/runtime.css") {
    return [...paths];
  }

  const normalized = normalizeWorkspaceAssetPath(relativePath);

  for (const root of collectPreviewWorkspaceRoots()) {
    paths.add(`${root}${normalized}`);

    if (normalized.endsWith("/")) {
      paths.add(`${root}${normalized}index.html`);
    }
  }

  return [...paths];
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

function collectPreviewWorkspaceRoots(): string[] {
  return [...new Set(PREVIEW_DOCUMENT_CANDIDATES.map((path) => dirname(path)))];
}

function dirname(path: string): string {
  const normalized = path.replace(/\/+$/, "");
  const index = normalized.lastIndexOf("/");
  return index <= 0 ? "/workspace" : normalized.slice(0, index);
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

function decodeWorkspacePath(relativePath: string): string {
  const encodedSuffix = relativePath.replace(/^\/files/, "");
  const suffix = encodedSuffix
    .split("/")
    .map((segment) => decodeURIComponent(segment))
    .join("/");

  return `/workspace${suffix}`;
}

function normalizeWorkspaceAssetPath(relativePath: string): string {
  const normalized = relativePath.startsWith("/") ? relativePath : `/${relativePath}`;
  return normalized.replace(/\/+/g, "/");
}
