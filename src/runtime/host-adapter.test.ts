import { expect, test } from "vite-plus/test";

import { MockRuntimeHostAdapter } from "./host-adapter";
import type { WorkspaceFileRecord } from "./analyze-archive";
import type { SessionSnapshot } from "./protocol";

const session: SessionSnapshot = {
  sessionId: "session-1",
  state: "mounted",
  workspaceRoot: "/workspace",
  archive: {
    fileName: "demo.zip",
    fileCount: 2,
    directoryCount: 1,
    entries: [],
    rootPrefix: "demo",
  },
  packageJson: {
    name: "demo-app",
    scripts: {
      dev: "vite",
    },
    dependencies: ["react"],
    devDependencies: ["typescript"],
  },
  capabilities: {
    detectedReact: true,
    detectedVite: true,
  },
};

const files = new Map<string, WorkspaceFileRecord>([
  [
    "/workspace/package.json",
    {
      path: "/workspace/package.json",
      size: 19,
      contentType: "application/json; charset=utf-8",
      isText: true,
      bytes: new TextEncoder().encode('{"name":"demo-app"}'),
      textContent: '{"name":"demo-app"}',
    },
  ],
  [
    "/workspace/logo.png",
    {
      path: "/workspace/logo.png",
      size: 4,
      contentType: "image/png",
      isText: false,
      bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47]),
      textContent: null,
    },
  ],
  [
    "/workspace/src/server.ts",
    {
      path: "/workspace/src/server.ts",
      size: 20,
      contentType: "text/plain; charset=utf-8",
      isText: true,
      bytes: new TextEncoder().encode("console.log('server')"),
      textContent: "console.log('server')",
    },
  ],
]);

test("MockRuntimeHostAdapter reports the null engine boot summary", async () => {
  const adapter = new MockRuntimeHostAdapter();

  await expect(adapter.bootSummary()).resolves.toEqual({
    engineName: "null-engine",
    supportsInterrupts: true,
    supportsModuleLoader: true,
    workspaceRoot: "/workspace",
  });
});

test("MockRuntimeHostAdapter creates sessions and returns run plans", async () => {
  const adapter = new MockRuntimeHostAdapter();

  await expect(
    adapter.createSession({
      sessionId: session.sessionId,
      session,
      files,
    }),
  ).resolves.toEqual({
    sessionId: "session-1",
    workspaceRoot: "/workspace",
    packageName: "demo-app",
    fileCount: 3,
  });

  await expect(
    adapter.planRun(session.sessionId, {
      cwd: "/workspace",
      command: "npm",
      args: ["run", "dev"],
      env: {
        NODE_ENV: "development",
      },
    }),
  ).resolves.toEqual({
    cwd: "/workspace",
    entrypoint: "dev",
    commandLine: "npm run dev",
    envCount: 1,
    commandKind: "npm-script",
    resolvedScript: "vite",
  });

  await expect(
    adapter.buildProcessInfo(session.sessionId, {
      cwd: "/workspace",
      command: "npm",
      args: ["run", "dev", "--host"],
      env: {
        NODE_ENV: "development",
      },
    }),
  ).resolves.toEqual({
    cwd: "/workspace",
    argv: ["/virtual/node", "npm", "run", "dev", "--host"],
    env: {
      NODE_ENV: "development",
    },
    execPath: "/virtual/node",
    platform: "browser",
    entrypoint: "dev",
    commandLine: "npm run dev --host",
    commandKind: "npm-script",
  });

  await expect(adapter.listWorkspaceFiles(session.sessionId)).resolves.toEqual([
    {
      path: "/workspace/package.json",
      size: 19,
      isText: true,
    },
    {
      path: "/workspace/logo.png",
      size: 4,
      isText: false,
    },
    {
      path: "/workspace/src/server.ts",
      size: 20,
      isText: true,
    },
  ]);

  await expect(adapter.statWorkspacePath(session.sessionId, "/workspace/src")).resolves.toEqual({
    path: "/workspace/src",
    kind: "directory",
    size: 0,
    isText: false,
  });

  await expect(adapter.readWorkspaceDirectory(session.sessionId, "/workspace")).resolves.toEqual([
    {
      path: "/workspace/logo.png",
      kind: "file",
      size: 4,
      isText: false,
    },
    {
      path: "/workspace/package.json",
      kind: "file",
      size: 19,
      isText: true,
    },
    {
      path: "/workspace/src",
      kind: "directory",
      size: 0,
      isText: false,
    },
  ]);

  await expect(adapter.resolvePreviewRequestHint(session.sessionId, "/logo.png")).resolves.toEqual({
    kind: "workspace-asset",
    workspacePath: "/workspace/logo.png",
    documentRoot: "/workspace",
    hydratePaths: ["/workspace/package.json", "/workspace/logo.png"],
  });

  await expect(
    adapter.readWorkspaceFile(session.sessionId, "/workspace/package.json"),
  ).resolves.toEqual({
    path: "/workspace/package.json",
    size: 19,
    isText: true,
    textContent: '{"name":"demo-app"}',
    bytes: new TextEncoder().encode('{"name":"demo-app"}'),
  });

  await expect(
    adapter.readWorkspaceFiles(session.sessionId, [
      "/workspace/package.json",
      "/workspace/logo.png",
    ]),
  ).resolves.toEqual([
    {
      path: "/workspace/package.json",
      size: 19,
      isText: true,
      textContent: '{"name":"demo-app"}',
      bytes: new TextEncoder().encode('{"name":"demo-app"}'),
    },
    {
      path: "/workspace/logo.png",
      size: 4,
      isText: false,
      textContent: null,
      bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47]),
    },
  ]);
});

test("MockRuntimeHostAdapter resolves document-root preview assets", async () => {
  const adapter = new MockRuntimeHostAdapter();
  const distFiles = new Map<string, WorkspaceFileRecord>([
    [
      "/workspace/dist/index.html",
      {
        path: "/workspace/dist/index.html",
        size: 51,
        contentType: "text/html; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode('<script type="module" src="/assets/app.js"></script>'),
        textContent: '<script type="module" src="/assets/app.js"></script>',
      },
    ],
    [
      "/workspace/dist/assets/app.js",
      {
        path: "/workspace/dist/assets/app.js",
        size: 20,
        contentType: "text/javascript; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode("console.log('dist');"),
        textContent: "console.log('dist');",
      },
    ],
    [
      "/workspace/server.js",
      {
        path: "/workspace/server.js",
        size: 21,
        contentType: "text/javascript; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode("console.log('server');"),
        textContent: "console.log('server');",
      },
    ],
  ]);

  await adapter.createSession({
    sessionId: "session-dist",
    session: {
      ...session,
      sessionId: "session-dist",
    },
    files: distFiles,
  });

  await expect(adapter.resolvePreviewRequestHint("session-dist", "/")).resolves.toEqual({
    kind: "root-document",
    workspacePath: "/workspace/dist/index.html",
    documentRoot: "/workspace/dist",
    hydratePaths: ["/workspace/dist/index.html"],
  });

  await expect(
    adapter.resolvePreviewRequestHint("session-dist", "/assets/app.js"),
  ).resolves.toEqual({
    kind: "workspace-asset",
    workspacePath: "/workspace/dist/assets/app.js",
    documentRoot: "/workspace/dist",
    hydratePaths: ["/workspace/dist/assets/app.js"],
  });

  await expect(
    adapter.planRun("session-dist", {
      cwd: "/workspace",
      command: "node",
      args: ["server.js"],
    }),
  ).resolves.toEqual({
    cwd: "/workspace",
    entrypoint: "/workspace/server.js",
    commandLine: "node server.js",
    envCount: 0,
    commandKind: "node-entrypoint",
    resolvedScript: null,
  });
});

test("MockRuntimeHostAdapter validates cwd and node entrypoints", async () => {
  const adapter = new MockRuntimeHostAdapter();

  await adapter.createSession({
    sessionId: session.sessionId,
    session,
    files,
  });

  await expect(
    adapter.planRun(session.sessionId, {
      cwd: "src",
      command: "node",
      args: ["server"],
    }),
  ).resolves.toEqual({
    cwd: "/workspace/src",
    entrypoint: "/workspace/src/server.ts",
    commandLine: "node server",
    envCount: 0,
    commandKind: "node-entrypoint",
    resolvedScript: null,
  });

  await expect(
    adapter.planRun(session.sessionId, {
      cwd: "/tmp",
      command: "node",
      args: ["server"],
    }),
  ).rejects.toThrow("working directory must stay under /workspace");

  await expect(
    adapter.planRun(session.sessionId, {
      cwd: "/workspace",
      command: "node",
      args: ["missing-entry"],
    }),
  ).rejects.toThrow("entrypoint not found");

  await expect(
    adapter.planRun(session.sessionId, {
      cwd: "/workspace/package.json",
      command: "node",
      args: ["server"],
    }),
  ).rejects.toThrow("workspace path is not a directory");

  await expect(
    adapter.planRun(session.sessionId, {
      cwd: "/workspace/missing",
      command: "node",
      args: ["server"],
    }),
  ).rejects.toThrow("workspace directory not found");
});

test("MockRuntimeHostAdapter mutates the workspace tree", async () => {
  const adapter = new MockRuntimeHostAdapter();

  await adapter.createSession({
    sessionId: session.sessionId,
    session,
    files,
  });

  await expect(
    adapter.createWorkspaceDirectory(session.sessionId, "/workspace/src/generated"),
  ).resolves.toEqual({
    path: "/workspace/src/generated",
    kind: "directory",
    size: 0,
    isText: false,
  });

  await expect(
    adapter.writeWorkspaceFile(session.sessionId, {
      path: "/workspace/src/generated/app.ts",
      bytes: new TextEncoder().encode("export const generated = true;"),
      isText: true,
    }),
  ).resolves.toEqual({
    path: "/workspace/src/generated/app.ts",
    kind: "file",
    size: 30,
    isText: true,
  });

  await expect(
    adapter.readWorkspaceDirectory(session.sessionId, "/workspace/src"),
  ).resolves.toEqual([
    {
      path: "/workspace/src/generated",
      kind: "directory",
      size: 0,
      isText: false,
    },
    {
      path: "/workspace/src/server.ts",
      kind: "file",
      size: 20,
      isText: true,
    },
  ]);

  await expect(
    adapter.readWorkspaceFile(session.sessionId, "/workspace/src/generated/app.ts"),
  ).resolves.toEqual({
    path: "/workspace/src/generated/app.ts",
    size: 30,
    isText: true,
    textContent: "export const generated = true;",
    bytes: new TextEncoder().encode("export const generated = true;"),
  });

  await expect(
    adapter.writeWorkspaceFile(session.sessionId, {
      path: "/workspace/src",
      bytes: new TextEncoder().encode("nope"),
      isText: true,
    }),
  ).rejects.toThrow("workspace path is a directory");

  await expect(adapter.createWorkspaceDirectory(session.sessionId, "/tmp/outside")).rejects.toThrow(
    "workspace path must stay under /workspace",
  );
});

test("MockRuntimeHostAdapter exposes a generic fs command surface", async () => {
  const adapter = new MockRuntimeHostAdapter();

  await adapter.createSession({
    sessionId: session.sessionId,
    session,
    files,
  });

  await expect(
    adapter.executeFsCommand(session.sessionId, {
      kind: "exists",
      cwd: "/workspace/src",
      path: "server.ts",
    }),
  ).resolves.toEqual({
    kind: "exists",
    path: "/workspace/src/server.ts",
    exists: true,
  });

  await expect(
    adapter.executeFsCommand(session.sessionId, {
      kind: "read-file",
      cwd: "/workspace",
      path: "package.json",
    }),
  ).resolves.toEqual({
    kind: "file",
    path: "/workspace/package.json",
    size: 19,
    isText: true,
    textContent: '{"name":"demo-app"}',
    bytes: new TextEncoder().encode('{"name":"demo-app"}'),
  });

  await expect(
    adapter.executeFsCommand(session.sessionId, {
      kind: "mkdir",
      cwd: "/workspace/src",
      path: "generated/nested",
    }),
  ).resolves.toEqual({
    kind: "entry",
    entry: {
      path: "/workspace/src/generated/nested",
      kind: "directory",
      size: 0,
      isText: false,
    },
  });

  await expect(
    adapter.executeFsCommand(session.sessionId, {
      kind: "write-file",
      cwd: "/workspace/src",
      path: "generated/nested/runtime.txt",
      bytes: new TextEncoder().encode("runtime host"),
      isText: true,
    }),
  ).resolves.toEqual({
    kind: "entry",
    entry: {
      path: "/workspace/src/generated/nested/runtime.txt",
      kind: "file",
      size: 12,
      isText: true,
    },
  });

  const runtimeContext = await adapter.createRuntimeContext(session.sessionId, {
    cwd: "/workspace/src",
    command: "node",
    args: ["server"],
  });

  await expect(
    adapter.executeContextFsCommand(runtimeContext.contextId, {
      kind: "read-file",
      path: "server.ts",
    }),
  ).resolves.toEqual({
    kind: "file",
    path: "/workspace/src/server.ts",
    size: 20,
    isText: true,
    textContent: "console.log('server')",
    bytes: new TextEncoder().encode("console.log('server')"),
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.describe",
    }),
  ).resolves.toEqual({
    kind: "runtime-bindings",
    bindings: {
      contextId: runtimeContext.contextId,
      engineName: "null-engine",
      entrypoint: "/workspace/src/server.ts",
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
          commandPrefixes: ["timers"],
        },
        {
          name: "console",
          globals: ["console"],
          modules: ["console", "node:console"],
          commandPrefixes: ["console"],
        },
      ],
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "stdio.write",
      stream: "stdout",
      chunk: "hello stdout",
    }),
  ).resolves.toEqual({
    kind: "event-queued",
    queueLen: 1,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "console.emit",
      level: "warn",
      values: ["watch", "out"],
    }),
  ).resolves.toEqual({
    kind: "event-queued",
    queueLen: 3,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.drain-events",
    }),
  ).resolves.toEqual({
    kind: "runtime-events",
    events: [
      {
        kind: "stdout",
        chunk: "hello stdout",
      },
      {
        kind: "console",
        level: "warn",
        line: "watch out",
      },
      {
        kind: "stderr",
        chunk: "watch out",
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "port.listen",
      protocol: "http",
    }),
  ).resolves.toEqual({
    kind: "port-listening",
    port: {
      port: 3000,
      protocol: "http",
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "port.listen",
      port: 4100,
      protocol: "http",
    }),
  ).resolves.toEqual({
    kind: "port-listening",
    port: {
      port: 4100,
      protocol: "http",
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "port.list",
    }),
  ).resolves.toEqual({
    kind: "port-list",
    ports: [
      {
        port: 3000,
        protocol: "http",
      },
      {
        port: 4100,
        protocol: "http",
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "http.serve-preview",
      port: 4200,
    }),
  ).resolves.toEqual({
    kind: "http-server-listening",
    server: {
      port: {
        port: 4200,
        protocol: "http",
      },
      kind: "preview",
      cwd: "/workspace/src",
      entrypoint: "/workspace/src/server.ts",
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "http.list-servers",
    }),
  ).resolves.toEqual({
    kind: "http-server-list",
    servers: [
      {
        port: {
          port: 4200,
          protocol: "http",
        },
        kind: "preview",
        cwd: "/workspace/src",
        entrypoint: "/workspace/src/server.ts",
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "http.resolve-preview",
      request: {
        port: 4200,
        method: "GET",
        relativePath: "/src/server.ts",
        search: "?v=1",
      },
    }),
  ).resolves.toEqual({
    kind: "preview-request-resolved",
    server: {
      port: {
        port: 4200,
        protocol: "http",
      },
      kind: "preview",
      cwd: "/workspace/src",
      entrypoint: "/workspace/src/server.ts",
    },
    port: {
      port: 4200,
      protocol: "http",
    },
    request: {
      port: 4200,
      method: "GET",
      relativePath: "/src/server.ts",
      search: "?v=1",
    },
    requestHint: {
      kind: "workspace-asset",
      workspacePath: "/workspace/src/server.ts",
      documentRoot: "/workspace",
      hydratePaths: ["/workspace/package.json", "/workspace/src/server.ts"],
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "http.close-server",
      port: 4200,
    }),
  ).resolves.toEqual({
    kind: "http-server-closed",
    port: 4200,
    existed: true,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.drain-events",
    }),
  ).resolves.toEqual({
    kind: "runtime-events",
    events: [
      {
        kind: "port-listen",
        port: {
          port: 3000,
          protocol: "http",
        },
      },
      {
        kind: "port-listen",
        port: {
          port: 4100,
          protocol: "http",
        },
      },
      {
        kind: "port-listen",
        port: {
          port: 4200,
          protocol: "http",
        },
      },
      {
        kind: "port-close",
        port: 4200,
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "port.close",
      port: 3000,
    }),
  ).resolves.toEqual({
    kind: "port-closed",
    port: 3000,
    existed: true,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.drain-events",
    }),
  ).resolves.toEqual({
    kind: "runtime-events",
    events: [
      {
        kind: "port-close",
        port: 3000,
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "timers.schedule",
      delayMs: 50,
      repeat: false,
    }),
  ).resolves.toEqual({
    kind: "timer-scheduled",
    timer: {
      timerId: "runtime-timer-1",
      kind: "timeout",
      delayMs: 50,
      dueAtMs: 50,
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "timers.list",
    }),
  ).resolves.toEqual({
    kind: "timer-list",
    nowMs: 0,
    timers: [
      {
        timerId: "runtime-timer-1",
        kind: "timeout",
        delayMs: 50,
        dueAtMs: 50,
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "timers.advance",
      elapsedMs: 25,
    }),
  ).resolves.toEqual({
    kind: "timer-fired",
    nowMs: 25,
    timers: [],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "timers.advance",
      elapsedMs: 25,
    }),
  ).resolves.toEqual({
    kind: "timer-fired",
    nowMs: 50,
    timers: [
      {
        timerId: "runtime-timer-1",
        kind: "timeout",
        delayMs: 50,
        dueAtMs: 50,
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "timers.schedule",
      delayMs: 10,
      repeat: true,
    }),
  ).resolves.toEqual({
    kind: "timer-scheduled",
    timer: {
      timerId: "runtime-timer-2",
      kind: "interval",
      delayMs: 10,
      dueAtMs: 60,
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "timers.advance",
      elapsedMs: 35,
    }),
  ).resolves.toEqual({
    kind: "timer-fired",
    nowMs: 85,
    timers: [
      {
        timerId: "runtime-timer-2",
        kind: "interval",
        delayMs: 10,
        dueAtMs: 60,
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "timers.list",
    }),
  ).resolves.toEqual({
    kind: "timer-list",
    nowMs: 85,
    timers: [
      {
        timerId: "runtime-timer-2",
        kind: "interval",
        delayMs: 10,
        dueAtMs: 90,
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "timers.clear",
      timerId: "runtime-timer-2",
    }),
  ).resolves.toEqual({
    kind: "timer-cleared",
    timerId: "runtime-timer-2",
    existed: true,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "process.info",
    }),
  ).resolves.toEqual({
    kind: "process-info",
    process: {
      cwd: "/workspace/src",
      argv: ["/virtual/node", "/workspace/src/server.ts"],
      env: {},
      execPath: "/virtual/node",
      platform: "browser",
      entrypoint: "/workspace/src/server.ts",
      commandLine: "node server",
      commandKind: "node-entrypoint",
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "process.status",
    }),
  ).resolves.toEqual({
    kind: "process-status",
    exited: false,
    exitCode: null,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "process.chdir",
      path: "../src/generated/nested",
    }),
  ).resolves.toEqual({
    kind: "process-cwd",
    cwd: "/workspace/src/generated/nested",
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "fs.write-file",
      path: "context.log",
      bytes: new TextEncoder().encode("context write"),
      isText: true,
    }),
  ).resolves.toEqual({
    kind: "fs",
    response: {
      kind: "entry",
      entry: {
        path: "/workspace/src/generated/nested/context.log",
        kind: "file",
        size: 13,
        isText: true,
      },
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "path.resolve",
      segments: ["../package.json"],
    }),
  ).resolves.toEqual({
    kind: "path-value",
    value: "/workspace/src/generated/package.json",
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "path.join",
      segments: ["/workspace", "src", "..", "logo.png"],
    }),
  ).resolves.toEqual({
    kind: "path-value",
    value: "/workspace/logo.png",
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "path.extname",
      path: "/workspace/src/generated/nested/context.log",
    }),
  ).resolves.toEqual({
    kind: "path-value",
    value: ".log",
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "process.exit",
      code: 0,
    }),
  ).resolves.toEqual({
    kind: "process-status",
    exited: true,
    exitCode: 0,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.drain-events",
    }),
  ).resolves.toEqual({
    kind: "runtime-events",
    events: [
      {
        kind: "process-exit",
        code: 0,
      },
    ],
  });

  await expect(adapter.dropRuntimeContext(runtimeContext.contextId)).resolves.toBeUndefined();

  await expect(
    adapter.executeContextFsCommand(runtimeContext.contextId, {
      kind: "read-file",
      path: "server.ts",
    }),
  ).rejects.toThrow("runtime context not found");
});
