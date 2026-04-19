import { guessContentType, type WorkspaceFileRecord } from "./analyze-archive";
import type { RunRequest, SessionSnapshot } from "./protocol";

export type HostBootstrapSummary = {
  engineName: string;
  supportsInterrupts: boolean;
  supportsModuleLoader: boolean;
  workspaceRoot: string;
};

export type HostRunPlan = {
  cwd: string;
  entrypoint: string;
  commandLine: string;
  envCount: number;
  commandKind: "npm-script" | "node-entrypoint";
  resolvedScript: string | null;
};

export type HostSessionHandle = {
  sessionId: string;
  workspaceRoot: string;
  packageName: string | null;
  fileCount: number;
};

export type HostWorkspaceFileSummary = {
  path: string;
  size: number;
  isText: boolean;
};

export type HostWorkspaceEntrySummary = {
  path: string;
  kind: "file" | "directory";
  size: number;
  isText: boolean;
};

export type HostWorkspaceFileContent = HostWorkspaceFileSummary & {
  textContent: string | null;
  bytes: Uint8Array;
};

type MockPreviewRootHint =
  | {
      kind: "workspace-document";
      path: string;
      root: string;
    }
  | {
      kind: "source-entry";
      path: string;
      root: null;
    }
  | {
      kind: "fallback";
      path: null;
      root: null;
    };

export type HostPreviewRequestHint =
  | {
      kind: "root-document";
      workspacePath: string;
      documentRoot: string;
      hydratePaths: string[];
    }
  | {
      kind: "root-entry";
      workspacePath: string;
      documentRoot: null;
      hydratePaths: string[];
    }
  | {
      kind:
        | "fallback-root"
        | "runtime-state"
        | "workspace-state"
        | "file-index"
        | "diagnostics-state"
        | "runtime-stylesheet"
        | "not-found";
      workspacePath: null;
      documentRoot: null;
      hydratePaths: string[];
    }
  | {
      kind: "workspace-file" | "workspace-asset";
      workspacePath: string;
      documentRoot: string;
      hydratePaths: string[];
    };

export interface RuntimeHostAdapter {
  bootSummary(): Promise<HostBootstrapSummary>;
  createSession(input: {
    sessionId: string;
    session: SessionSnapshot;
    files: Map<string, WorkspaceFileRecord>;
  }): Promise<HostSessionHandle>;
  planRun(sessionId: string, request: RunRequest): Promise<HostRunPlan>;
  listWorkspaceFiles(sessionId: string): Promise<HostWorkspaceFileSummary[]>;
  statWorkspacePath(sessionId: string, path: string): Promise<HostWorkspaceEntrySummary>;
  readWorkspaceDirectory(sessionId: string, path: string): Promise<HostWorkspaceEntrySummary[]>;
  createWorkspaceDirectory(sessionId: string, path: string): Promise<HostWorkspaceEntrySummary>;
  writeWorkspaceFile(
    sessionId: string,
    input: {
      path: string;
      bytes: Uint8Array;
      isText: boolean;
    },
  ): Promise<HostWorkspaceEntrySummary>;
  resolvePreviewRequestHint(
    sessionId: string,
    relativePath: string,
  ): Promise<HostPreviewRequestHint>;
  readWorkspaceFile(sessionId: string, path: string): Promise<HostWorkspaceFileContent>;
  readWorkspaceFiles(sessionId: string, paths: string[]): Promise<HostWorkspaceFileContent[]>;
  stopSession(sessionId: string): Promise<void>;
}

type WasmRuntimeHostExports = {
  memory: WebAssembly.Memory;
  runtime_host_alloc(len: number): number;
  runtime_host_dealloc(ptr: number, len: number): void;
  runtime_host_last_result_ptr(): number;
  runtime_host_last_result_len(): number;
  runtime_host_boot_summary_json(): number;
  runtime_host_create_session_json(ptr: number, len: number): number;
  runtime_host_plan_run_json(ptr: number, len: number): number;
  runtime_host_list_workspace_files_json(ptr: number, len: number): number;
  runtime_host_stat_workspace_path_json(ptr: number, len: number): number;
  runtime_host_read_workspace_directory_json(ptr: number, len: number): number;
  runtime_host_create_workspace_directory_json(ptr: number, len: number): number;
  runtime_host_write_workspace_file_json(ptr: number, len: number): number;
  runtime_host_resolve_preview_request_hint_json(ptr: number, len: number): number;
  runtime_host_read_workspace_file_json(ptr: number, len: number): number;
  runtime_host_read_workspace_files_json(ptr: number, len: number): number;
  runtime_host_stop_session_json(ptr: number, len: number): number;
};

const DEFAULT_RUNTIME_HOST_WASM_URL = "/runtime-host.wasm";

type HostSessionRecord = {
  handle: HostSessionHandle;
  packageScripts: Record<string, string>;
  files: Map<string, WorkspaceFileRecord>;
  directories: Set<string>;
};

export class MockRuntimeHostAdapter implements RuntimeHostAdapter {
  private readonly sessions = new Map<string, HostSessionRecord>();

  async bootSummary(): Promise<HostBootstrapSummary> {
    return {
      engineName: "null-engine",
      supportsInterrupts: true,
      supportsModuleLoader: true,
      workspaceRoot: "/workspace",
    };
  }

  async createSession(input: {
    sessionId: string;
    session: SessionSnapshot;
    files: Map<string, WorkspaceFileRecord>;
  }): Promise<HostSessionHandle> {
    const handle: HostSessionHandle = {
      sessionId: input.sessionId,
      workspaceRoot: input.session.workspaceRoot,
      packageName: input.session.packageJson?.name ?? null,
      fileCount: input.files.size,
    };

    this.sessions.set(input.sessionId, {
      handle,
      packageScripts: input.session.packageJson?.scripts ?? {},
      files: input.files,
      directories: collectMockDirectories(input.files, input.session.workspaceRoot),
    });

    return handle;
  }

  async planRun(sessionId: string, request: RunRequest): Promise<HostRunPlan> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const commandLine = [request.command, ...request.args].join(" ").trim();
    const cwd = resolveMockRunCwd(
      record.handle.workspaceRoot,
      record.directories,
      record.files,
      request.cwd,
    );

    if (request.command === "npm" && request.args[0] === "run") {
      const scriptName = request.args[1];

      if (!scriptName || !(scriptName in record.packageScripts)) {
        throw new Error(`script not found: ${scriptName ?? "<missing>"}`);
      }

      return {
        cwd,
        entrypoint: scriptName,
        commandLine,
        envCount: Object.keys(request.env ?? {}).length,
        commandKind: "npm-script",
        resolvedScript: record.packageScripts[scriptName] ?? null,
      };
    }

    if (request.command === "node") {
      const entrypoint = resolveMockNodeEntrypoint(record.files, cwd, request.args[0]);

      return {
        cwd,
        entrypoint,
        commandLine,
        envCount: Object.keys(request.env ?? {}).length,
        commandKind: "node-entrypoint",
        resolvedScript: null,
      };
    }

    throw new Error(`unsupported command: ${commandLine || request.command || "<empty>"}`);
  }

  async listWorkspaceFiles(sessionId: string): Promise<HostWorkspaceFileSummary[]> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    return [...record.files.values()].map((file) => ({
      path: file.path,
      size: file.size,
      isText: file.isText,
    }));
  }

  async statWorkspacePath(sessionId: string, path: string): Promise<HostWorkspaceEntrySummary> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const resolved = resolveMockWorkspacePath(record.handle.workspaceRoot, path);
    const file = record.files.get(resolved);
    if (file) {
      return {
        path: file.path,
        kind: "file",
        size: file.size,
        isText: file.isText,
      };
    }

    if (record.directories.has(resolved)) {
      return {
        path: resolved,
        kind: "directory",
        size: 0,
        isText: false,
      };
    }

    throw new Error(`workspace file not found: ${resolved}`);
  }

  async readWorkspaceDirectory(
    sessionId: string,
    path: string,
  ): Promise<HostWorkspaceEntrySummary[]> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const resolved = resolveMockWorkspacePath(record.handle.workspaceRoot, path);
    if (!record.directories.has(resolved)) {
      if (record.files.has(resolved)) {
        throw new Error(`workspace path is not a directory: ${resolved}`);
      }

      throw new Error(`workspace directory not found: ${resolved}`);
    }

    const entries = new Map<string, HostWorkspaceEntrySummary>();
    for (const directory of record.directories) {
      if (directory !== resolved && parentMockPosixPath(directory) === resolved) {
        entries.set(directory, {
          path: directory,
          kind: "directory",
          size: 0,
          isText: false,
        });
      }
    }

    for (const file of record.files.values()) {
      if (parentMockPosixPath(file.path) === resolved) {
        entries.set(file.path, {
          path: file.path,
          kind: "file",
          size: file.size,
          isText: file.isText,
        });
      }
    }

    return [...entries.values()].sort((left, right) => left.path.localeCompare(right.path));
  }

  async createWorkspaceDirectory(
    sessionId: string,
    path: string,
  ): Promise<HostWorkspaceEntrySummary> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const resolved = resolveMockWorkspacePath(record.handle.workspaceRoot, path);
    assertMockWorkspacePathWithinRoot(record.handle.workspaceRoot, resolved);
    if (record.files.has(resolved)) {
      throw new Error(`workspace path is not a directory: ${resolved}`);
    }

    let current = resolved;
    while (current.startsWith(record.handle.workspaceRoot)) {
      record.directories.add(current);

      if (current === record.handle.workspaceRoot) {
        break;
      }

      current = parentMockPosixPath(current);
    }

    return {
      path: resolved,
      kind: "directory",
      size: 0,
      isText: false,
    };
  }

  async writeWorkspaceFile(
    sessionId: string,
    input: {
      path: string;
      bytes: Uint8Array;
      isText: boolean;
    },
  ): Promise<HostWorkspaceEntrySummary> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const resolved = resolveMockWorkspacePath(record.handle.workspaceRoot, input.path);
    assertMockWorkspacePathWithinRoot(record.handle.workspaceRoot, resolved);
    if (record.directories.has(resolved)) {
      throw new Error(`workspace path is a directory: ${resolved}`);
    }

    let current = parentMockPosixPath(resolved);
    while (current.startsWith(record.handle.workspaceRoot)) {
      record.directories.add(current);

      if (current === record.handle.workspaceRoot) {
        break;
      }

      current = parentMockPosixPath(current);
    }

    const bytes = new Uint8Array(input.bytes);
    const textContent = input.isText ? new TextDecoder().decode(bytes) : null;
    record.files.set(resolved, {
      path: resolved,
      size: bytes.byteLength,
      contentType: guessContentType(resolved),
      isText: input.isText,
      bytes,
      textContent,
    });

    return {
      path: resolved,
      kind: "file",
      size: bytes.byteLength,
      isText: input.isText,
    };
  }

  private getPreviewRootHint(sessionId: string): MockPreviewRootHint {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    for (const candidate of [
      "/workspace/index.html",
      "/workspace/dist/index.html",
      "/workspace/build/index.html",
      "/workspace/public/index.html",
    ]) {
      const file = record.files.get(candidate);

      if (file && file.isText && file.contentType.startsWith("text/html")) {
        return {
          kind: "workspace-document",
          path: file.path,
          root: candidate.slice(0, candidate.lastIndexOf("/")) || "/workspace",
        };
      }
    }

    for (const candidate of [
      "/workspace/src/main.tsx",
      "/workspace/src/main.jsx",
      "/workspace/src/main.ts",
      "/workspace/src/main.js",
      "/workspace/src/index.tsx",
      "/workspace/src/index.jsx",
      "/workspace/src/index.ts",
      "/workspace/src/index.js",
    ]) {
      if (record.files.has(candidate)) {
        return {
          kind: "source-entry",
          path: candidate,
          root: null,
        };
      }
    }

    return {
      kind: "fallback",
      path: null,
      root: null,
    };
  }

  async resolvePreviewRequestHint(
    sessionId: string,
    relativePath: string,
  ): Promise<HostPreviewRequestHint> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const hydrationPaths = collectMockPreviewHydrationPaths(record.files);

    if (relativePath === "/" || relativePath === "/index.html") {
      const rootHint = this.getPreviewRootHint(sessionId);

      if (rootHint.kind === "workspace-document") {
        return {
          kind: "root-document",
          workspacePath: rootHint.path,
          documentRoot: rootHint.root,
          hydratePaths: [...hydrationPaths, rootHint.path],
        };
      }

      if (rootHint.kind === "source-entry") {
        return {
          kind: "root-entry",
          workspacePath: rootHint.path,
          documentRoot: null,
          hydratePaths: [...hydrationPaths, rootHint.path],
        };
      }

      return {
        kind: "fallback-root",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: hydrationPaths,
      };
    }

    if (relativePath === "/__runtime.json") {
      return { kind: "runtime-state", workspacePath: null, documentRoot: null, hydratePaths: [] };
    }

    if (relativePath === "/__workspace.json") {
      return { kind: "workspace-state", workspacePath: null, documentRoot: null, hydratePaths: [] };
    }

    if (relativePath === "/__files.json") {
      return { kind: "file-index", workspacePath: null, documentRoot: null, hydratePaths: [] };
    }

    if (relativePath === "/__diagnostics.json") {
      return {
        kind: "diagnostics-state",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      };
    }

    if (relativePath === "/assets/runtime.css") {
      return {
        kind: "runtime-stylesheet",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      };
    }

    if (relativePath.startsWith("/files/")) {
      const workspacePath = `/workspace${relativePath.replace(/^\/files/, "")}`;

      if (record.files.has(workspacePath)) {
        return {
          kind: "workspace-file",
          workspacePath,
          documentRoot: "/workspace",
          hydratePaths: [...hydrationPaths, workspacePath],
        };
      }

      return { kind: "not-found", workspacePath: null, documentRoot: null, hydratePaths: [] };
    }

    const rootHint = this.getPreviewRootHint(sessionId);
    const documentRoot = rootHint.kind === "workspace-document" ? rootHint.root : "/workspace";
    const workspacePath = resolveMockPreviewAssetWorkspacePath(
      record.files,
      relativePath,
      documentRoot,
    );

    if (workspacePath) {
      return {
        kind: "workspace-asset",
        workspacePath,
        documentRoot,
        hydratePaths: [...hydrationPaths, workspacePath],
      };
    }

    return { kind: "not-found", workspacePath: null, documentRoot: null, hydratePaths: [] };
  }

  async readWorkspaceFile(sessionId: string, path: string): Promise<HostWorkspaceFileContent> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const file = record.files.get(path);

    if (!file) {
      throw new Error(`workspace file not found: ${path}`);
    }

    return {
      path: file.path,
      size: file.size,
      isText: file.isText,
      textContent: file.textContent,
      bytes: file.bytes,
    };
  }

  async readWorkspaceFiles(
    sessionId: string,
    paths: string[],
  ): Promise<HostWorkspaceFileContent[]> {
    return Promise.all(paths.map((path) => this.readWorkspaceFile(sessionId, path)));
  }

  async stopSession(sessionId: string): Promise<void> {
    this.sessions.delete(sessionId);
  }
}

export class WasmRuntimeHostAdapter implements RuntimeHostAdapter {
  private readonly exports: WasmRuntimeHostExports;
  private readonly encoder = new TextEncoder();
  private readonly decoder = new TextDecoder();

  private constructor(exports: WasmRuntimeHostExports) {
    this.exports = exports;
  }

  static async create(wasmUrl = DEFAULT_RUNTIME_HOST_WASM_URL): Promise<WasmRuntimeHostAdapter> {
    const response = await fetch(wasmUrl);

    if (!response.ok) {
      throw new Error(`Failed to load runtime host wasm: ${response.status}`);
    }

    const bytes = await response.arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, {});

    return new WasmRuntimeHostAdapter(instance.exports as unknown as WasmRuntimeHostExports);
  }

  async bootSummary(): Promise<HostBootstrapSummary> {
    return this.invokeWithoutInput<HostBootstrapSummary>("runtime_host_boot_summary_json");
  }

  async createSession(input: {
    sessionId: string;
    session: SessionSnapshot;
    files: Map<string, WorkspaceFileRecord>;
  }): Promise<HostSessionHandle> {
    return this.invokeWithInput<HostSessionHandle>("runtime_host_create_session_json", [
      `session_id=${input.sessionId}`,
      `archive_file_name=${input.session.archive.fileName}`,
      `file_count=${input.session.archive.fileCount}`,
      `directory_count=${input.session.archive.directoryCount}`,
      `root_prefix=${input.session.archive.rootPrefix ?? ""}`,
      `package_name=${input.session.packageJson?.name ?? ""}`,
      `package_scripts=${serializeStringMap(input.session.packageJson?.scripts ?? {})}`,
      `workspace_root=${input.session.workspaceRoot}`,
      `detected_react=${String(input.session.capabilities.detectedReact)}`,
      `detected_vite=${String(input.session.capabilities.detectedVite)}`,
      `files=${serializeWorkspaceFiles(input.files)}`,
    ]);
  }

  async planRun(sessionId: string, request: RunRequest): Promise<HostRunPlan> {
    const args = request.args.join("\u001f");
    const env = Object.entries(request.env ?? {})
      .map(([key, value]) => `${key}=${value}`)
      .join("\u001f");

    return this.invokeWithInput<HostRunPlan>("runtime_host_plan_run_json", [
      `session_id=${sessionId}`,
      `cwd=${request.cwd}`,
      `command=${request.command}`,
      `args=${args}`,
      `env=${env}`,
    ]);
  }

  async stopSession(sessionId: string): Promise<void> {
    await this.invokeWithInput<{ sessionId: string }>("runtime_host_stop_session_json", [
      `session_id=${sessionId}`,
    ]);
  }

  async listWorkspaceFiles(sessionId: string): Promise<HostWorkspaceFileSummary[]> {
    return this.invokeWithInput<HostWorkspaceFileSummary[]>(
      "runtime_host_list_workspace_files_json",
      [`session_id=${sessionId}`],
    );
  }

  async statWorkspacePath(sessionId: string, path: string): Promise<HostWorkspaceEntrySummary> {
    return this.invokeWithInput<HostWorkspaceEntrySummary>(
      "runtime_host_stat_workspace_path_json",
      [`session_id=${sessionId}`, `path=${path}`],
    );
  }

  async readWorkspaceDirectory(
    sessionId: string,
    path: string,
  ): Promise<HostWorkspaceEntrySummary[]> {
    return this.invokeWithInput<HostWorkspaceEntrySummary[]>(
      "runtime_host_read_workspace_directory_json",
      [`session_id=${sessionId}`, `path=${path}`],
    );
  }

  async createWorkspaceDirectory(
    sessionId: string,
    path: string,
  ): Promise<HostWorkspaceEntrySummary> {
    return this.invokeWithInput<HostWorkspaceEntrySummary>(
      "runtime_host_create_workspace_directory_json",
      [`session_id=${sessionId}`, `path=${encodeHex(path)}`],
    );
  }

  async writeWorkspaceFile(
    sessionId: string,
    input: {
      path: string;
      bytes: Uint8Array;
      isText: boolean;
    },
  ): Promise<HostWorkspaceEntrySummary> {
    return this.invokeWithInput<HostWorkspaceEntrySummary>(
      "runtime_host_write_workspace_file_json",
      [
        `session_id=${sessionId}`,
        `path=${encodeHex(input.path)}`,
        `is_text=${String(input.isText)}`,
        `bytes=${encodeHex(input.bytes)}`,
      ],
    );
  }

  async resolvePreviewRequestHint(
    sessionId: string,
    relativePath: string,
  ): Promise<HostPreviewRequestHint> {
    return this.invokeWithInput<HostPreviewRequestHint>(
      "runtime_host_resolve_preview_request_hint_json",
      [`session_id=${sessionId}`, `relative_path=${encodeHex(relativePath)}`],
    );
  }

  async readWorkspaceFile(sessionId: string, path: string): Promise<HostWorkspaceFileContent> {
    const file = await this.invokeWithInput<{
      path: string;
      size: number;
      isText: boolean;
      textContent: string | null;
      bytesHex: string;
    }>("runtime_host_read_workspace_file_json", [
      `session_id=${sessionId}`,
      `path=${encodeHex(path)}`,
    ]);

    return {
      path: file.path,
      size: file.size,
      isText: file.isText,
      textContent: file.textContent,
      bytes: decodeHex(file.bytesHex),
    };
  }

  async readWorkspaceFiles(
    sessionId: string,
    paths: string[],
  ): Promise<HostWorkspaceFileContent[]> {
    if (paths.length === 0) {
      return [];
    }

    const files = await this.invokeWithInput<
      Array<{
        path: string;
        size: number;
        isText: boolean;
        textContent: string | null;
        bytesHex: string;
      }>
    >("runtime_host_read_workspace_files_json", [
      `session_id=${sessionId}`,
      `paths=${paths.map((path) => encodeHex(path)).join("\u001f")}`,
    ]);

    return files.map((file) => ({
      path: file.path,
      size: file.size,
      isText: file.isText,
      textContent: file.textContent,
      bytes: decodeHex(file.bytesHex),
    }));
  }

  private async invokeWithoutInput<T>(exportName: keyof WasmRuntimeHostExports): Promise<T> {
    const exported = this.exports[exportName];

    if (typeof exported !== "function") {
      throw new Error(`WASM export is not callable: ${String(exportName)}`);
    }

    const callable = exported as () => number;
    callable();
    return this.readResult<T>();
  }

  private async invokeWithInput<T>(
    exportName: keyof WasmRuntimeHostExports,
    lines: string[],
  ): Promise<T> {
    const exported = this.exports[exportName];

    if (typeof exported !== "function") {
      throw new Error(`WASM export is not callable: ${String(exportName)}`);
    }

    const callable = exported as (ptr: number, len: number) => number;

    const payload = this.encoder.encode(lines.join("\n"));
    const ptr = this.exports.runtime_host_alloc(payload.byteLength);

    try {
      if (payload.byteLength > 0) {
        new Uint8Array(this.exports.memory.buffer, ptr, payload.byteLength).set(payload);
      }

      callable(ptr, payload.byteLength);
      return this.readResult<T>();
    } finally {
      if (ptr !== 0 && payload.byteLength > 0) {
        this.exports.runtime_host_dealloc(ptr, payload.byteLength);
      }
    }
  }

  private readResult<T>(): T {
    const ptr = this.exports.runtime_host_last_result_ptr();
    const len = this.exports.runtime_host_last_result_len();
    const bytes = new Uint8Array(this.exports.memory.buffer, ptr, len);
    const text = this.decoder.decode(bytes);
    const parsed = JSON.parse(text) as T & { error?: string };

    if ("error" in parsed && typeof parsed.error === "string") {
      throw new Error(parsed.error);
    }

    return parsed;
  }
}

function collectMockPreviewHydrationPaths(files: Map<string, WorkspaceFileRecord>): string[] {
  return [...files.values()]
    .filter((file) => file.path.endsWith("/package.json"))
    .map((file) => file.path);
}

function resolveMockPreviewAssetWorkspacePath(
  files: Map<string, WorkspaceFileRecord>,
  relativePath: string,
  documentRoot: string,
): string | null {
  if (relativePath.startsWith("/__") || relativePath === "/assets/runtime.css") {
    return null;
  }

  const normalized = (relativePath.startsWith("/") ? relativePath : `/${relativePath}`).replace(
    /\/+/g,
    "/",
  );
  const candidates = [`${documentRoot}${normalized}`, `/workspace${normalized}`];

  if (normalized.endsWith("/")) {
    candidates.push(`${documentRoot}${normalized}index.html`, `/workspace${normalized}index.html`);
  }

  return candidates.find((candidate) => files.has(candidate)) ?? null;
}

function resolveMockRunCwd(
  workspaceRoot: string,
  directories: Set<string>,
  files: Map<string, WorkspaceFileRecord>,
  cwd: string,
): string {
  const normalized = resolveMockWorkspacePath(workspaceRoot, cwd);

  if (!(normalized === workspaceRoot || normalized.startsWith(`${workspaceRoot}/`))) {
    throw new Error(`working directory must stay under /workspace: ${normalized}`);
  }

  if (!directories.has(normalized)) {
    if (files.has(normalized)) {
      throw new Error(`workspace path is not a directory: ${normalized}`);
    }

    throw new Error(`workspace directory not found: ${normalized}`);
  }

  return normalized;
}

function resolveMockNodeEntrypoint(
  files: Map<string, WorkspaceFileRecord>,
  cwd: string,
  entrypoint: string | undefined,
): string {
  if (!entrypoint) {
    throw new Error("node entrypoint is required");
  }

  const requested = normalizeMockPosixPath(
    entrypoint.startsWith("/") ? entrypoint : `${cwd}/${entrypoint}`,
  );
  const candidates = [
    requested,
    `${requested}.js`,
    `${requested}.mjs`,
    `${requested}.cjs`,
    `${requested}.ts`,
    `${requested}.tsx`,
    `${requested}.jsx`,
    `${requested}/index.js`,
    `${requested}/index.ts`,
    `${requested}/index.tsx`,
  ];

  for (const candidate of candidates) {
    if (files.has(candidate)) {
      return candidate;
    }
  }

  throw new Error(`entrypoint not found: ${requested}`);
}

function normalizeMockPosixPath(input: string): string {
  const isAbsolute = input.startsWith("/");
  const segments = input.split("/").filter(Boolean);
  const normalized: string[] = [];

  for (const segment of segments) {
    if (segment === ".") {
      continue;
    }

    if (segment === "..") {
      normalized.pop();
      continue;
    }

    normalized.push(segment);
  }

  if (normalized.length === 0) {
    return isAbsolute ? "/" : ".";
  }

  return `${isAbsolute ? "/" : ""}${normalized.join("/")}`;
}

function resolveMockWorkspacePath(workspaceRoot: string, path: string): string {
  if (!path) {
    return workspaceRoot;
  }

  return normalizeMockPosixPath(path.startsWith("/") ? path : `${workspaceRoot}/${path}`);
}

function assertMockWorkspacePathWithinRoot(workspaceRoot: string, path: string): void {
  if (!(path === workspaceRoot || path.startsWith(`${workspaceRoot}/`))) {
    throw new Error(`workspace path must stay under /workspace: ${path}`);
  }
}

function collectMockDirectories(
  files: Map<string, WorkspaceFileRecord>,
  workspaceRoot: string,
): Set<string> {
  const directories = new Set<string>([workspaceRoot]);

  for (const path of files.keys()) {
    let current = parentMockPosixPath(path);

    while (current.startsWith(workspaceRoot)) {
      directories.add(current);

      if (current === workspaceRoot) {
        break;
      }

      current = parentMockPosixPath(current);
    }
  }

  return directories;
}

function parentMockPosixPath(path: string): string {
  const normalized = path.replace(/\/+$/, "");

  if (!normalized || normalized === "/") {
    return "/";
  }

  const index = normalized.lastIndexOf("/");
  if (index <= 0) {
    return "/";
  }

  return normalized.slice(0, index);
}

export async function createRuntimeHostAdapter(): Promise<RuntimeHostAdapter> {
  try {
    return await WasmRuntimeHostAdapter.create();
  } catch {
    return new MockRuntimeHostAdapter();
  }
}

function serializeWorkspaceFiles(files: Map<string, WorkspaceFileRecord>): string {
  return [...files.values()]
    .map((file) =>
      [encodeHex(file.path), file.isText ? "1" : "0", encodeHex(file.bytes)].join("\u001f"),
    )
    .join("\u001e");
}

function serializeStringMap(values: Record<string, string>): string {
  return Object.entries(values)
    .map(([key, value]) => [encodeHex(key), encodeHex(value)].join("\u001f"))
    .join("\u001e");
}

function encodeHex(input: string | Uint8Array): string {
  const bytes = typeof input === "string" ? new TextEncoder().encode(input) : input;
  let result = "";

  for (const byte of bytes) {
    result += byte.toString(16).padStart(2, "0");
  }

  return result;
}

function decodeHex(input: string): Uint8Array {
  const bytes = new Uint8Array(input.length / 2);

  for (let index = 0; index < input.length; index += 2) {
    bytes[index / 2] = Number.parseInt(input.slice(index, index + 2), 16);
  }

  return bytes;
}
