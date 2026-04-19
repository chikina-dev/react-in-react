import type { WorkspaceFileRecord } from "./analyze-archive";
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

export type HostWorkspaceFileContent = HostWorkspaceFileSummary & {
  textContent: string | null;
  bytes: Uint8Array;
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
  resolvePreviewHydrationPaths(sessionId: string, relativePath: string): Promise<string[]>;
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
  runtime_host_resolve_preview_hydration_paths_json(ptr: number, len: number): number;
  runtime_host_read_workspace_file_json(ptr: number, len: number): number;
  runtime_host_read_workspace_files_json(ptr: number, len: number): number;
  runtime_host_stop_session_json(ptr: number, len: number): number;
};

const DEFAULT_RUNTIME_HOST_WASM_URL = "/runtime-host.wasm";

type HostSessionRecord = {
  handle: HostSessionHandle;
  files: Map<string, WorkspaceFileRecord>;
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
      files: input.files,
    });

    return handle;
  }

  async planRun(sessionId: string, request: RunRequest): Promise<HostRunPlan> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const commandLine = [request.command, ...request.args].join(" ").trim();

    return {
      cwd: request.cwd || record.handle.workspaceRoot,
      entrypoint: request.command,
      commandLine,
      envCount: Object.keys(request.env ?? {}).length,
    };
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

  async resolvePreviewHydrationPaths(sessionId: string, relativePath: string): Promise<string[]> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const paths = new Set<string>();

    for (const file of record.files.values()) {
      if (file.path.endsWith("/package.json")) {
        paths.add(file.path);
      }
    }

    if (relativePath === "/" || relativePath === "/index.html") {
      for (const candidate of [
        "/workspace/index.html",
        "/workspace/dist/index.html",
        "/workspace/build/index.html",
        "/workspace/public/index.html",
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
          paths.add(candidate);
        }
      }

      return [...paths];
    }

    if (relativePath.startsWith("/files/")) {
      const workspacePath = `/workspace${relativePath.replace(/^\/files/, "")}`;

      if (record.files.has(workspacePath)) {
        paths.add(workspacePath);
      }

      return [...paths];
    }

    if (relativePath.startsWith("/__") || relativePath === "/assets/runtime.css") {
      return [...paths];
    }

    const normalized = (relativePath.startsWith("/") ? relativePath : `/${relativePath}`).replace(
      /\/+/g,
      "/",
    );

    for (const root of ["/workspace", "/workspace/dist", "/workspace/build", "/workspace/public"]) {
      const candidate = `${root}${normalized}`;

      if (record.files.has(candidate)) {
        paths.add(candidate);
      }

      if (normalized.endsWith("/")) {
        const indexCandidate = `${root}${normalized}index.html`;

        if (record.files.has(indexCandidate)) {
          paths.add(indexCandidate);
        }
      }
    }

    return [...paths];
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

  async resolvePreviewHydrationPaths(sessionId: string, relativePath: string): Promise<string[]> {
    return this.invokeWithInput<string[]>("runtime_host_resolve_preview_hydration_paths_json", [
      `session_id=${sessionId}`,
      `relative_path=${encodeHex(relativePath)}`,
    ]);
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
