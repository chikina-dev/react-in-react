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

export type HostProcessInfo = {
  cwd: string;
  argv: string[];
  env: Record<string, string>;
  execPath: string;
  platform: string;
  entrypoint: string;
  commandLine: string;
  commandKind: "npm-script" | "node-entrypoint";
};

export type HostRuntimeContext = {
  contextId: string;
  sessionId: string;
  process: HostProcessInfo;
};

export type HostRuntimeBuiltinSpec = {
  name: string;
  globals: string[];
  modules: string[];
  commandPrefixes: string[];
};

export type HostRuntimeBindings = {
  contextId: string;
  engineName: string;
  entrypoint: string;
  globals: string[];
  builtins: HostRuntimeBuiltinSpec[];
};

export type HostRuntimeCommand =
  | { kind: "runtime.describe" | "process.info" | "process.cwd" | "process.argv" | "process.env" }
  | { kind: "process.chdir"; path: string }
  | { kind: "path.resolve" | "path.join"; segments: string[] }
  | { kind: "path.dirname" | "path.basename" | "path.extname" | "path.normalize"; path: string }
  | (
      | {
          kind: "fs.exists" | "fs.stat" | "fs.read-dir" | "fs.read-file" | "fs.mkdir";
          path: string;
        }
      | { kind: "fs.write-file"; path: string; bytes: Uint8Array; isText: boolean }
    );

export type HostRuntimeResponse =
  | { kind: "runtime-bindings"; bindings: HostRuntimeBindings }
  | { kind: "process-info"; process: HostProcessInfo }
  | { kind: "process-cwd"; cwd: string }
  | { kind: "process-argv"; argv: string[] }
  | { kind: "process-env"; env: Record<string, string> }
  | { kind: "path-value"; value: string }
  | { kind: "fs"; response: HostFsResponse };

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

export type HostFsCommand =
  | {
      kind: "exists" | "stat" | "read-dir" | "read-file" | "mkdir";
      cwd?: string;
      path: string;
    }
  | {
      kind: "write-file";
      cwd?: string;
      path: string;
      bytes: Uint8Array;
      isText: boolean;
    };

export type HostContextFsCommand =
  | {
      kind: "exists" | "stat" | "read-dir" | "read-file" | "mkdir";
      path: string;
    }
  | {
      kind: "write-file";
      path: string;
      bytes: Uint8Array;
      isText: boolean;
    };

export type HostFsResponse =
  | {
      kind: "exists";
      path: string;
      exists: boolean;
    }
  | {
      kind: "entry";
      entry: HostWorkspaceEntrySummary;
    }
  | {
      kind: "directory-entries";
      entries: HostWorkspaceEntrySummary[];
    }
  | {
      kind: "file";
      path: string;
      size: number;
      isText: boolean;
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
  buildProcessInfo(sessionId: string, request: RunRequest): Promise<HostProcessInfo>;
  createRuntimeContext(sessionId: string, request: RunRequest): Promise<HostRuntimeContext>;
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
  executeFsCommand(sessionId: string, command: HostFsCommand): Promise<HostFsResponse>;
  executeContextFsCommand(
    contextId: string,
    command: HostContextFsCommand,
  ): Promise<HostFsResponse>;
  executeRuntimeCommand(
    contextId: string,
    command: HostRuntimeCommand,
  ): Promise<HostRuntimeResponse>;
  dropRuntimeContext(contextId: string): Promise<void>;
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
  runtime_host_build_process_info_json(ptr: number, len: number): number;
  runtime_host_create_runtime_context_json(ptr: number, len: number): number;
  runtime_host_list_workspace_files_json(ptr: number, len: number): number;
  runtime_host_stat_workspace_path_json(ptr: number, len: number): number;
  runtime_host_read_workspace_directory_json(ptr: number, len: number): number;
  runtime_host_create_workspace_directory_json(ptr: number, len: number): number;
  runtime_host_write_workspace_file_json(ptr: number, len: number): number;
  runtime_host_execute_fs_command_json(ptr: number, len: number): number;
  runtime_host_execute_context_fs_command_json(ptr: number, len: number): number;
  runtime_host_execute_runtime_command_json(ptr: number, len: number): number;
  runtime_host_drop_runtime_context_json(ptr: number, len: number): number;
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

type HostRuntimeContextRecord = {
  contextId: string;
  sessionId: string;
  process: HostProcessInfo;
};

export class MockRuntimeHostAdapter implements RuntimeHostAdapter {
  private readonly sessions = new Map<string, HostSessionRecord>();
  private readonly runtimeContexts = new Map<string, HostRuntimeContextRecord>();
  private nextRuntimeContextId = 1;

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

  async buildProcessInfo(sessionId: string, request: RunRequest): Promise<HostProcessInfo> {
    const runPlan = await this.planRun(sessionId, request);
    const argv =
      runPlan.commandKind === "node-entrypoint"
        ? ["/virtual/node", runPlan.entrypoint, ...request.args.slice(1)]
        : ["/virtual/node", "npm", "run", runPlan.entrypoint, ...request.args.slice(2)];

    return {
      cwd: runPlan.cwd,
      argv,
      env: request.env ?? {},
      execPath: "/virtual/node",
      platform: "browser",
      entrypoint: runPlan.entrypoint,
      commandLine: runPlan.commandLine,
      commandKind: runPlan.commandKind,
    };
  }

  async createRuntimeContext(sessionId: string, request: RunRequest): Promise<HostRuntimeContext> {
    const process = await this.buildProcessInfo(sessionId, request);
    const contextId = `runtime-context-${this.nextRuntimeContextId++}`;
    const context = {
      contextId,
      sessionId,
      process,
    };
    this.runtimeContexts.set(contextId, context);
    return context;
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

  async executeFsCommand(sessionId: string, command: HostFsCommand): Promise<HostFsResponse> {
    const record = this.sessions.get(sessionId);

    if (!record) {
      throw new Error(`Rust host session not found: ${sessionId}`);
    }

    const resolvedCwd = resolveMockRunCwd(
      record.handle.workspaceRoot,
      record.directories,
      record.files,
      command.cwd ?? record.handle.workspaceRoot,
    );

    switch (command.kind) {
      case "exists": {
        const resolved = resolveMockFsCommandPath(resolvedCwd, command.path);
        assertMockWorkspacePathWithinRoot(record.handle.workspaceRoot, resolved);

        return {
          kind: "exists",
          path: resolved,
          exists: record.files.has(resolved) || record.directories.has(resolved),
        };
      }
      case "stat":
        return {
          kind: "entry",
          entry: await this.statWorkspacePath(
            sessionId,
            resolveMockFsCommandPath(resolvedCwd, command.path),
          ),
        };
      case "read-dir":
        return {
          kind: "directory-entries",
          entries: await this.readWorkspaceDirectory(
            sessionId,
            resolveMockFsCommandPath(resolvedCwd, command.path),
          ),
        };
      case "read-file": {
        const file = await this.readWorkspaceFile(
          sessionId,
          resolveMockFsCommandPath(resolvedCwd, command.path),
        );
        return {
          kind: "file",
          path: file.path,
          size: file.size,
          isText: file.isText,
          textContent: file.textContent,
          bytes: file.bytes,
        };
      }
      case "mkdir":
        return {
          kind: "entry",
          entry: await this.createWorkspaceDirectory(
            sessionId,
            resolveMockFsCommandPath(resolvedCwd, command.path),
          ),
        };
      case "write-file":
        return {
          kind: "entry",
          entry: await this.writeWorkspaceFile(sessionId, {
            path: resolveMockFsCommandPath(resolvedCwd, command.path),
            bytes: command.bytes,
            isText: command.isText,
          }),
        };
    }
  }

  async executeContextFsCommand(
    contextId: string,
    command: HostContextFsCommand,
  ): Promise<HostFsResponse> {
    const context = this.runtimeContexts.get(contextId);

    if (!context) {
      throw new Error(`runtime context not found: ${contextId}`);
    }

    return this.executeFsCommand(context.sessionId, {
      ...command,
      cwd: context.process.cwd,
    });
  }

  async executeRuntimeCommand(
    contextId: string,
    command: HostRuntimeCommand,
  ): Promise<HostRuntimeResponse> {
    const context = this.runtimeContexts.get(contextId);

    if (!context) {
      throw new Error(`runtime context not found: ${contextId}`);
    }

    switch (command.kind) {
      case "runtime.describe":
        return {
          kind: "runtime-bindings",
          bindings: {
            contextId,
            engineName: "null-engine",
            entrypoint: context.process.entrypoint,
            globals: ["console", "process", "Buffer", "setTimeout", "clearTimeout", "__runtime"],
            builtins: [
              {
                name: "process",
                globals: ["process"],
                modules: ["process", "node:process"],
                commandPrefixes: ["process"],
              },
              {
                name: "fs",
                globals: [],
                modules: ["fs", "node:fs"],
                commandPrefixes: ["fs"],
              },
              {
                name: "path",
                globals: [],
                modules: ["path", "node:path"],
                commandPrefixes: ["path"],
              },
              {
                name: "buffer",
                globals: ["Buffer"],
                modules: ["buffer", "node:buffer"],
                commandPrefixes: [],
              },
              {
                name: "timers",
                globals: ["setTimeout", "clearTimeout"],
                modules: ["timers", "node:timers"],
                commandPrefixes: [],
              },
              {
                name: "console",
                globals: ["console"],
                modules: ["console", "node:console"],
                commandPrefixes: [],
              },
            ],
          },
        };
      case "process.info":
        return {
          kind: "process-info",
          process: {
            ...context.process,
            argv: [...context.process.argv],
            env: { ...context.process.env },
          },
        };
      case "process.cwd":
        return {
          kind: "process-cwd",
          cwd: context.process.cwd,
        };
      case "process.argv":
        return {
          kind: "process-argv",
          argv: [...context.process.argv],
        };
      case "process.env":
        return {
          kind: "process-env",
          env: { ...context.process.env },
        };
      case "process.chdir": {
        const record = this.sessions.get(context.sessionId);

        if (!record) {
          throw new Error(`Rust host session not found: ${context.sessionId}`);
        }

        const nextCwd = resolveMockRunCwd(
          record.handle.workspaceRoot,
          record.directories,
          record.files,
          resolveMockFsCommandPath(context.process.cwd, command.path),
        );
        context.process.cwd = nextCwd;
        return {
          kind: "process-cwd",
          cwd: nextCwd,
        };
      }
      case "path.resolve":
        return {
          kind: "path-value",
          value: resolveMockRuntimePath(context.process.cwd, command.segments),
        };
      case "path.join":
        return {
          kind: "path-value",
          value: joinMockRuntimePath(command.segments),
        };
      case "path.dirname":
        return {
          kind: "path-value",
          value: dirnameMockRuntimePath(command.path),
        };
      case "path.basename":
        return {
          kind: "path-value",
          value: basenameMockRuntimePath(command.path),
        };
      case "path.extname":
        return {
          kind: "path-value",
          value: extnameMockRuntimePath(command.path),
        };
      case "path.normalize":
        return {
          kind: "path-value",
          value: normalizeMockPosixPath(command.path),
        };
      case "fs.exists":
      case "fs.stat":
      case "fs.read-dir":
      case "fs.read-file":
      case "fs.mkdir":
      case "fs.write-file": {
        const fsCommand: HostContextFsCommand =
          command.kind === "fs.write-file"
            ? {
                kind: "write-file",
                path: command.path,
                bytes: command.bytes,
                isText: command.isText,
              }
            : {
                kind:
                  command.kind === "fs.exists"
                    ? "exists"
                    : command.kind === "fs.stat"
                      ? "stat"
                      : command.kind === "fs.read-dir"
                        ? "read-dir"
                        : command.kind === "fs.read-file"
                          ? "read-file"
                          : "mkdir",
                path: command.path,
              };

        return {
          kind: "fs",
          response: await this.executeContextFsCommand(contextId, fsCommand),
        };
      }
    }
  }

  async dropRuntimeContext(contextId: string): Promise<void> {
    if (!this.runtimeContexts.delete(contextId)) {
      throw new Error(`runtime context not found: ${contextId}`);
    }
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
    for (const [contextId, context] of this.runtimeContexts.entries()) {
      if (context.sessionId === sessionId) {
        this.runtimeContexts.delete(contextId);
      }
    }
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

  async buildProcessInfo(sessionId: string, request: RunRequest): Promise<HostProcessInfo> {
    const args = request.args.join("\u001f");
    const env = Object.entries(request.env ?? {})
      .map(([key, value]) => `${key}=${value}`)
      .join("\u001f");

    return this.invokeWithInput<HostProcessInfo>("runtime_host_build_process_info_json", [
      `session_id=${sessionId}`,
      `cwd=${request.cwd}`,
      `command=${request.command}`,
      `args=${args}`,
      `env=${env}`,
    ]);
  }

  async createRuntimeContext(sessionId: string, request: RunRequest): Promise<HostRuntimeContext> {
    const args = request.args.join("\u001f");
    const env = Object.entries(request.env ?? {})
      .map(([key, value]) => `${key}=${value}`)
      .join("\u001f");

    return this.invokeWithInput<HostRuntimeContext>("runtime_host_create_runtime_context_json", [
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

  async executeFsCommand(sessionId: string, command: HostFsCommand): Promise<HostFsResponse> {
    const lines = [
      `session_id=${sessionId}`,
      `command=${command.kind}`,
      `cwd=${encodeHex(command.cwd ?? "/workspace")}`,
    ];

    if ("path" in command) {
      lines.push(`path=${encodeHex(command.path)}`);
    }

    if (command.kind === "write-file") {
      lines.push(`is_text=${String(command.isText)}`);
      lines.push(`bytes=${encodeHex(command.bytes)}`);
    }

    const response = await this.invokeWithInput<
      | {
          kind: "exists";
          path: string;
          exists: boolean;
        }
      | {
          kind: "entry";
          entry: HostWorkspaceEntrySummary;
        }
      | {
          kind: "directory-entries";
          entries: HostWorkspaceEntrySummary[];
        }
      | {
          kind: "file";
          path: string;
          size: number;
          isText: boolean;
          textContent: string | null;
          bytesHex: string;
        }
    >("runtime_host_execute_fs_command_json", lines);

    if (response.kind === "file") {
      return {
        kind: "file",
        path: response.path,
        size: response.size,
        isText: response.isText,
        textContent: response.textContent,
        bytes: decodeHex(response.bytesHex),
      };
    }

    return response;
  }

  async executeContextFsCommand(
    contextId: string,
    command: HostContextFsCommand,
  ): Promise<HostFsResponse> {
    const lines = [`context_id=${contextId}`, `command=${command.kind}`];

    if ("path" in command) {
      lines.push(`path=${encodeHex(command.path)}`);
    }

    if (command.kind === "write-file") {
      lines.push(`is_text=${String(command.isText)}`);
      lines.push(`bytes=${encodeHex(command.bytes)}`);
    }

    const response = await this.invokeWithInput<
      | {
          kind: "exists";
          path: string;
          exists: boolean;
        }
      | {
          kind: "entry";
          entry: HostWorkspaceEntrySummary;
        }
      | {
          kind: "directory-entries";
          entries: HostWorkspaceEntrySummary[];
        }
      | {
          kind: "file";
          path: string;
          size: number;
          isText: boolean;
          textContent: string | null;
          bytesHex: string;
        }
    >("runtime_host_execute_context_fs_command_json", lines);

    if (response.kind === "file") {
      return {
        kind: "file",
        path: response.path,
        size: response.size,
        isText: response.isText,
        textContent: response.textContent,
        bytes: decodeHex(response.bytesHex),
      };
    }

    return response;
  }

  async executeRuntimeCommand(
    contextId: string,
    command: HostRuntimeCommand,
  ): Promise<HostRuntimeResponse> {
    const lines = [`context_id=${contextId}`];

    switch (command.kind) {
      case "runtime.describe":
        lines.push("command=runtime-describe");
        break;
      case "process.info":
        lines.push("command=process-info");
        break;
      case "process.cwd":
        lines.push("command=process-cwd");
        break;
      case "process.argv":
        lines.push("command=process-argv");
        break;
      case "process.env":
        lines.push("command=process-env");
        break;
      case "process.chdir":
        lines.push("command=process-chdir");
        lines.push(`path=${encodeHex(command.path)}`);
        break;
      case "path.resolve":
        lines.push("command=path-resolve");
        lines.push(
          `segments=${command.segments.map((segment) => encodeHex(segment)).join("\u001f")}`,
        );
        break;
      case "path.join":
        lines.push("command=path-join");
        lines.push(
          `segments=${command.segments.map((segment) => encodeHex(segment)).join("\u001f")}`,
        );
        break;
      case "path.dirname":
        lines.push("command=path-dirname");
        lines.push(`path=${encodeHex(command.path)}`);
        break;
      case "path.basename":
        lines.push("command=path-basename");
        lines.push(`path=${encodeHex(command.path)}`);
        break;
      case "path.extname":
        lines.push("command=path-extname");
        lines.push(`path=${encodeHex(command.path)}`);
        break;
      case "path.normalize":
        lines.push("command=path-normalize");
        lines.push(`path=${encodeHex(command.path)}`);
        break;
      case "fs.exists":
      case "fs.stat":
      case "fs.read-dir":
      case "fs.read-file":
      case "fs.mkdir":
      case "fs.write-file":
        lines.push(`command=${command.kind.replace(".", "-")}`);
        lines.push(`path=${encodeHex(command.path)}`);
        if (command.kind === "fs.write-file") {
          lines.push(`is_text=${String(command.isText)}`);
          lines.push(`bytes=${encodeHex(command.bytes)}`);
        }
        break;
    }

    const response = await this.invokeWithInput<
      | { kind: "runtime-bindings"; bindings: HostRuntimeBindings }
      | { kind: "process-info"; process: HostProcessInfo }
      | { kind: "process-cwd"; cwd: string }
      | { kind: "process-argv"; argv: string[] }
      | { kind: "process-env"; env: Record<string, string> }
      | { kind: "path-value"; value: string }
      | {
          kind: "fs";
          response:
            | {
                kind: "exists";
                path: string;
                exists: boolean;
              }
            | {
                kind: "entry";
                entry: HostWorkspaceEntrySummary;
              }
            | {
                kind: "directory-entries";
                entries: HostWorkspaceEntrySummary[];
              }
            | {
                kind: "file";
                path: string;
                size: number;
                isText: boolean;
                textContent: string | null;
                bytesHex: string;
              };
        }
    >("runtime_host_execute_runtime_command_json", lines);

    if (response.kind === "fs" && response.response.kind === "file") {
      return {
        kind: "fs",
        response: {
          kind: "file",
          path: response.response.path,
          size: response.response.size,
          isText: response.response.isText,
          textContent: response.response.textContent,
          bytes: decodeHex(response.response.bytesHex),
        },
      };
    }

    return response as HostRuntimeResponse;
  }

  async dropRuntimeContext(contextId: string): Promise<void> {
    await this.invokeWithInput<{ contextId: string }>("runtime_host_drop_runtime_context_json", [
      `context_id=${contextId}`,
    ]);
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

function resolveMockFsCommandPath(cwd: string, path: string): string {
  if (!path) {
    return cwd;
  }

  return normalizeMockPosixPath(path.startsWith("/") ? path : `${cwd}/${path}`);
}

function resolveMockRuntimePath(cwd: string, segments: string[]): string {
  let resolved = cwd;

  for (const segment of segments) {
    if (!segment) {
      continue;
    }

    resolved = segment.startsWith("/")
      ? segment
      : resolved === "/"
        ? `/${segment}`
        : `${resolved}/${segment}`;
  }

  return normalizeMockPosixPath(resolved);
}

function joinMockRuntimePath(segments: string[]): string {
  const joined = segments.filter(Boolean).join("/");
  return joined ? normalizeMockPosixPath(joined) : ".";
}

function dirnameMockRuntimePath(path: string): string {
  const normalized = normalizeMockPosixPath(path);

  if (normalized === "/") {
    return "/";
  }

  const trimmed = normalized.replace(/\/+$/, "");
  const index = trimmed.lastIndexOf("/");
  if (index === -1) {
    return ".";
  }
  if (index === 0) {
    return "/";
  }
  return trimmed.slice(0, index);
}

function basenameMockRuntimePath(path: string): string {
  const normalized = normalizeMockPosixPath(path);

  if (normalized === "/") {
    return "/";
  }

  return normalized.replace(/\/+$/, "").split("/").at(-1) ?? ".";
}

function extnameMockRuntimePath(path: string): string {
  const basename = basenameMockRuntimePath(path);
  if (basename === "/" || basename === "." || basename === "..") {
    return "";
  }
  const index = basename.lastIndexOf(".");
  if (index <= 0) {
    return "";
  }
  return basename.slice(index);
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
