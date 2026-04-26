import { expect, test } from "vite-plus/test";

import { createRuntimeHostWasmImports, MockRuntimeHostAdapter } from "./host-adapter";
import type { WorkspaceFileRecord } from "./analyze-archive";
import type { SessionSnapshot } from "./protocol";

const session: SessionSnapshot = {
  sessionId: "session-1",
  state: "mounted",
  revision: 0,
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
  [
    "/workspace/src/boot.ts",
    {
      path: "/workspace/src/boot.ts",
      size: 25,
      contentType: "text/plain; charset=utf-8",
      isText: true,
      bytes: new TextEncoder().encode("export const boot = true;"),
      textContent: "export const boot = true;",
    },
  ],
  [
    "/workspace/node_modules/.bin/vite",
    {
      path: "/workspace/node_modules/.bin/vite",
      size: 7,
      contentType: "text/plain; charset=utf-8",
      isText: true,
      bytes: new TextEncoder().encode("vite.js"),
      textContent: "vite.js",
    },
  ],
  [
    "/workspace/node_modules/vite/package.json",
    {
      path: "/workspace/node_modules/vite/package.json",
      size: 34,
      contentType: "application/json; charset=utf-8",
      isText: true,
      bytes: new TextEncoder().encode('{"bin":{"vite":"bin/vite.js"}}'),
      textContent: '{"bin":{"vite":"bin/vite.js"}}',
    },
  ],
  [
    "/workspace/node_modules/vite/bin/vite.js",
    {
      path: "/workspace/node_modules/vite/bin/vite.js",
      size: 20,
      contentType: "text/plain; charset=utf-8",
      isText: true,
      bytes: new TextEncoder().encode("console.log('vite')"),
      textContent: "console.log('vite')",
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

test("createRuntimeHostWasmImports defines stubs for browser QuickJS extern imports", () => {
  const imports = createRuntimeHostWasmImports([
    { module: "env", name: "JS_NewRuntime", kind: "function" },
    { module: "env", name: "JS_Eval", kind: "function" },
    { module: "env", name: "js_malloc", kind: "function" },
  ]);

  expect(Object.keys(imports)).toEqual(["env"]);
  expect(typeof imports.env?.JS_NewRuntime).toBe("function");
  expect(typeof imports.env?.JS_Eval).toBe("function");
  expect(typeof imports.env?.js_malloc).toBe("function");
  const newRuntime = imports.env?.JS_NewRuntime as (() => never) | undefined;

  expect(() => newRuntime?.()).toThrow(
    "runtime-host wasm import env.JS_NewRuntime is not wired yet",
  );
});

test("createRuntimeHostWasmImports rejects unsupported import kinds", () => {
  expect(() =>
    createRuntimeHostWasmImports([{ module: "env", name: "memory", kind: "memory" }]),
  ).toThrow("runtime-host wasm import env.memory has unsupported kind memory");
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
    fileCount: 7,
    fileIndex: [
      { path: "/workspace/logo.png", size: 4, isText: false },
      { path: "/workspace/node_modules/.bin/vite", size: 7, isText: true },
      { path: "/workspace/node_modules/vite/bin/vite.js", size: 20, isText: true },
      { path: "/workspace/node_modules/vite/package.json", size: 34, isText: true },
      { path: "/workspace/package.json", size: 19, isText: true },
      { path: "/workspace/src/boot.ts", size: 25, isText: true },
      { path: "/workspace/src/server.ts", size: 20, isText: true },
    ],
    samplePath: "/workspace/logo.png",
    sampleSize: 4,
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
    adapter.writeWorkspaceFile(session.sessionId, {
      path: "/workspace/package.json",
      bytes: new TextEncoder().encode(
        JSON.stringify({
          name: "renamed-app",
          scripts: {
            dev: "vite",
            preview: "vite preview",
          },
          dependencies: {
            react: "^19.0.0",
          },
          devDependencies: {
            vite: "^8.0.0",
          },
        }),
      ),
      isText: true,
    }),
  ).resolves.toEqual({
    path: "/workspace/package.json",
    kind: "file",
    size: 143,
    isText: true,
  });

  await expect(
    adapter.planRun(session.sessionId, {
      cwd: "/workspace",
      command: "npm",
      args: ["run", "preview"],
      env: {},
    }),
  ).resolves.toEqual({
    cwd: "/workspace",
    entrypoint: "preview",
    commandLine: "npm run preview",
    envCount: 0,
    commandKind: "npm-script",
    resolvedScript: "vite preview",
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
    argv: ["/virtual/node", "/workspace/node_modules/vite/bin/vite.js", "--host"],
    env: {
      NODE_ENV: "development",
    },
    execPath: "/virtual/node",
    platform: "browser",
    entrypoint: "/workspace/node_modules/vite/bin/vite.js",
    commandLine: "npm run dev --host",
    commandKind: "npm-script",
  });

  await expect(adapter.listWorkspaceFiles(session.sessionId)).resolves.toEqual([
    {
      path: "/workspace/package.json",
      size: 143,
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
    {
      path: "/workspace/src/boot.ts",
      size: 25,
      isText: true,
    },
    {
      path: "/workspace/node_modules/.bin/vite",
      size: 7,
      isText: true,
    },
    {
      path: "/workspace/node_modules/vite/package.json",
      size: 34,
      isText: true,
    },
    {
      path: "/workspace/node_modules/vite/bin/vite.js",
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
      path: "/workspace/node_modules",
      kind: "directory",
      size: 0,
      isText: false,
    },
    {
      path: "/workspace/package.json",
      kind: "file",
      size: 143,
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
    hydratePaths: [
      "/workspace/package.json",
      "/workspace/node_modules/vite/package.json",
      "/workspace/logo.png",
    ],
  });

  await expect(
    adapter.readWorkspaceFile(session.sessionId, "/workspace/package.json"),
  ).resolves.toEqual({
    path: "/workspace/package.json",
    size: 143,
    isText: true,
    textContent:
      '{"name":"renamed-app","scripts":{"dev":"vite","preview":"vite preview"},"dependencies":{"react":"^19.0.0"},"devDependencies":{"vite":"^8.0.0"}}',
    bytes: new TextEncoder().encode(
      '{"name":"renamed-app","scripts":{"dev":"vite","preview":"vite preview"},"dependencies":{"react":"^19.0.0"},"devDependencies":{"vite":"^8.0.0"}}',
    ),
  });

  await expect(
    adapter.readWorkspaceFiles(session.sessionId, [
      "/workspace/package.json",
      "/workspace/logo.png",
    ]),
  ).resolves.toEqual([
    {
      path: "/workspace/package.json",
      size: 143,
      isText: true,
      textContent:
        '{"name":"renamed-app","scripts":{"dev":"vite","preview":"vite preview"},"dependencies":{"react":"^19.0.0"},"devDependencies":{"vite":"^8.0.0"}}',
      bytes: new TextEncoder().encode(
        '{"name":"renamed-app","scripts":{"dev":"vite","preview":"vite preview"},"dependencies":{"react":"^19.0.0"},"devDependencies":{"vite":"^8.0.0"}}',
      ),
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

test("MockRuntimeHostAdapter adds a second browser CLI package by registry entry only", async () => {
  const adapter = new MockRuntimeHostAdapter();
  const acmeSession: SessionSnapshot = {
    ...session,
    sessionId: "session-acme",
    packageJson: {
      name: "acme-app",
      scripts: {
        dev: "acme-dev",
      },
      dependencies: [],
      devDependencies: ["acme-dev"],
    },
  };
  const acmeFiles = new Map<string, WorkspaceFileRecord>([
    [
      "/workspace/package.json",
      {
        path: "/workspace/package.json",
        size: 19,
        contentType: "application/json; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode('{"name":"acme-app"}'),
        textContent: '{"name":"acme-app"}',
      },
    ],
    [
      "/workspace/node_modules/.bin/acme-dev",
      {
        path: "/workspace/node_modules/.bin/acme-dev",
        size: 11,
        contentType: "text/plain; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode("acme-dev.js"),
        textContent: "acme-dev.js",
      },
    ],
    [
      "/workspace/node_modules/acme-dev/package.json",
      {
        path: "/workspace/node_modules/acme-dev/package.json",
        size: 66,
        contentType: "application/json; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode('{"name":"acme-dev","bin":{"acme-dev":"bin/acme-dev.js"}}'),
        textContent: '{"name":"acme-dev","bin":{"acme-dev":"bin/acme-dev.js"}}',
      },
    ],
    [
      "/workspace/node_modules/acme-dev/bin/acme-dev.js",
      {
        path: "/workspace/node_modules/acme-dev/bin/acme-dev.js",
        size: 24,
        contentType: "text/plain; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode("console.log('acme-dev')"),
        textContent: "console.log('acme-dev')",
      },
    ],
  ]);

  await adapter.createSession({
    sessionId: acmeSession.sessionId,
    session: acmeSession,
    files: acmeFiles,
  });

  await expect(
    adapter.launchRuntime(
      acmeSession.sessionId,
      {
        cwd: "/workspace",
        command: "npm",
        args: ["run", "dev"],
      },
      {
        maxTurns: 16,
        port: 3400,
      },
    ),
  ).resolves.toEqual(
    expect.objectContaining({
      runPlan: expect.objectContaining({
        commandKind: "npm-script",
        commandLine: "npm run dev",
        resolvedScript: "acme-dev",
      }),
      startupStdout: expect.arrayContaining([
        "[browser-cli] runtime=browser-dev-server preview=http-server mode=dev",
      ]),
      previewReady: expect.objectContaining({
        url: "/preview/session-acme/3400/",
      }),
    }),
  );
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
      path: "/workspace/src/boot.ts",
      kind: "file",
      size: 25,
      isText: true,
    },
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
      globals: [
        "console",
        "process",
        "Buffer",
        "setTimeout",
        "clearTimeout",
        "setInterval",
        "clearInterval",
        "__runtime",
      ],
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
          globals: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"],
          modules: ["timers", "node:timers"],
          commandPrefixes: ["timers"],
        },
        {
          name: "console",
          globals: ["console"],
          modules: ["console", "node:console"],
          commandPrefixes: ["console"],
        },
        {
          name: "perf_hooks",
          globals: ["performance"],
          modules: ["node:perf_hooks"],
          commandPrefixes: [],
        },
        {
          name: "module",
          globals: [],
          modules: ["node:module"],
          commandPrefixes: [],
        },
        {
          name: "inspector",
          globals: [],
          modules: ["node:inspector"],
          commandPrefixes: [],
        },
      ],
    },
  });

  await expect(adapter.describeEngineContext(runtimeContext.contextId)).resolves.toEqual({
    engineSessionId: `null-engine-session:${runtimeContext.sessionId}`,
    engineContextId: `null-engine-context:${runtimeContext.contextId}`,
    sessionId: runtimeContext.sessionId,
    cwd: "/workspace/src",
    entrypoint: "/workspace/src/server.ts",
    argvLen: 2,
    envCount: 0,
    pendingJobs: 0,
    registeredModules: 0,
    bootstrapSpecifier: null,
    bridgeReady: false,
    moduleLoaderRoots: [],
    state: "booted",
  });

  await expect(
    adapter.evalEngineContext(runtimeContext.contextId, {
      filename: "/workspace/src/server.ts",
      source: "console.log('server')",
      asModule: false,
    }),
  ).resolves.toEqual({
    resultSummary: "null-engine skipped script eval for /workspace/src/server.ts (21 bytes)",
    pendingJobs: 0,
    state: "ready",
  });

  await expect(adapter.drainEngineJobs(runtimeContext.contextId)).resolves.toEqual({
    drainedJobs: 0,
    pendingJobs: 0,
  });

  await expect(adapter.interruptEngineContext(runtimeContext.contextId, "test")).resolves.toBe(
    undefined,
  );

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.describe-bootstrap",
    }),
  ).resolves.toEqual({
    kind: "runtime-bootstrap",
    plan: expect.objectContaining({
      contextId: runtimeContext.contextId,
      engineName: "null-engine",
      entrypoint: "/workspace/src/server.ts",
      bootstrapSpecifier: "runtime:bootstrap",
      modules: expect.arrayContaining([
        expect.objectContaining({
          specifier: "node:process",
          source: expect.stringContaining("process.cwd"),
        }),
        expect.objectContaining({
          specifier: "runtime:bootstrap",
          source: expect.stringContaining('import("/workspace/src/server.ts")'),
        }),
      ]),
    }),
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.describe-module-loader",
    }),
  ).resolves.toEqual({
    kind: "runtime-module-loader",
    plan: {
      contextId: runtimeContext.contextId,
      engineName: "null-engine",
      cwd: "/workspace/src",
      entrypoint: "/workspace/src/server.ts",
      workspaceRoot: "/workspace",
      entryModule: {
        requestedSpecifier: "/workspace/src/server.ts",
        resolvedSpecifier: "/workspace/src/server.ts",
        kind: "workspace",
        format: "module",
      },
      registeredSpecifiers: expect.arrayContaining(["node:process", "runtime:bootstrap"]),
      nodeModuleSearchRoots: ["/workspace/node_modules", "/workspace/src/node_modules"],
    },
  });

  const bootResponse = await adapter.executeRuntimeCommand(runtimeContext.contextId, {
    kind: "runtime.boot-engine",
  });
  expect(bootResponse).toMatchObject({
    kind: "runtime-engine-boot",
    report: {
      plan: expect.objectContaining({
        contextId: runtimeContext.contextId,
        bootstrapSpecifier: "runtime:bootstrap",
      }),
      loaderPlan: {
        contextId: runtimeContext.contextId,
        engineName: "null-engine",
        cwd: "/workspace/src",
        entrypoint: "/workspace/src/server.ts",
        workspaceRoot: "/workspace",
        entryModule: {
          requestedSpecifier: "/workspace/src/server.ts",
          resolvedSpecifier: "/workspace/src/server.ts",
          kind: "workspace",
          format: "module",
        },
        registeredSpecifiers: expect.arrayContaining(["node:process", "runtime:bootstrap"]),
        nodeModuleSearchRoots: ["/workspace/node_modules", "/workspace/src/node_modules"],
      },
      pendingJobs: 0,
      drainedJobs: 0,
    },
  });
  expect(
    bootResponse.kind === "runtime-engine-boot" ? bootResponse.report.resultSummary : "",
  ).toContain("null-engine skipped module eval for runtime:bootstrap");

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.startup",
      maxTurns: 16,
    }),
  ).resolves.toEqual({
    kind: "runtime-startup",
    report: {
      boot: expect.objectContaining({
        plan: expect.objectContaining({
          contextId: runtimeContext.contextId,
          bootstrapSpecifier: "runtime:bootstrap",
        }),
        loaderPlan: expect.objectContaining({
          entryModule: {
            requestedSpecifier: "/workspace/src/server.ts",
            resolvedSpecifier: "/workspace/src/server.ts",
            kind: "workspace",
            format: "module",
          },
        }),
      }),
      entryImportPlan: {
        requestSpecifier: "/workspace/src/server.ts",
        importer: null,
        resolvedModule: {
          requestedSpecifier: "/workspace/src/server.ts",
          resolvedSpecifier: "/workspace/src/server.ts",
          kind: "workspace",
          format: "module",
        },
        loadedModule: {
          resolvedSpecifier: "/workspace/src/server.ts",
          kind: "workspace",
          format: "module",
          source: expect.stringContaining("console.log('server')"),
        },
      },
      idle: {
        turns: 0,
        drainedJobs: 0,
        firedTimers: 0,
        nowMs: 0,
        pendingJobs: 0,
        pendingTimers: 0,
        exited: false,
        exitCode: null,
        reachedTurnLimit: false,
      },
      exited: false,
      exitCode: null,
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.launch-preview",
      maxTurns: 16,
      port: 3100,
    }),
  ).resolves.toEqual({
    kind: "runtime-preview-launch",
    report: {
      startup: expect.objectContaining({
        boot: expect.objectContaining({
          plan: expect.objectContaining({
            contextId: runtimeContext.contextId,
            bootstrapSpecifier: "runtime:bootstrap",
          }),
        }),
        exited: false,
        exitCode: null,
      }),
      rootReport: expect.objectContaining({
        server: {
          port: {
            port: 3100,
            protocol: "http",
          },
          kind: "preview",
          cwd: "/workspace/src",
          entrypoint: "/workspace/src/server.ts",
        },
        port: {
          port: 3100,
          protocol: "http",
        },
        request: {
          port: 3100,
          method: "GET",
          relativePath: "/",
          search: "",
          clientModules: [],
        },
        requestHint: expect.objectContaining({
          kind: "fallback-root",
        }),
        responseDescriptor: expect.objectContaining({
          kind: "host-managed-fallback",
        }),
        hydrationPaths: ["/workspace/package.json", "/workspace/node_modules/vite/package.json"],
        hydratedFiles: expect.arrayContaining([
          {
            path: "/workspace/package.json",
            size: 19,
            isText: true,
            textContent: '{"name":"demo-app"}',
            bytes: new TextEncoder().encode('{"name":"demo-app"}'),
          },
          {
            path: "/workspace/node_modules/vite/package.json",
            size: 34,
            isText: true,
            textContent: '{"bin":{"vite":"bin/vite.js"}}',
            bytes: new TextEncoder().encode('{"bin":{"vite":"bin/vite.js"}}'),
          },
        ]),
        directResponse: expect.objectContaining({
          status: 503,
          headers: expect.objectContaining({
            "content-type": "text/html; charset=utf-8",
          }),
        }),
        renderPlan: {
          kind: "host-managed-fallback",
          workspacePath: null,
          documentRoot: null,
        },
        modulePlan: null,
        transformKind: null,
      }),
    },
  });

  await expect(
    adapter.launchRuntime(
      session.sessionId,
      {
        cwd: "/workspace/src",
        command: "node",
        args: ["server.ts"],
      },
      {
        maxTurns: 16,
        port: 3200,
      },
    ),
  ).resolves.toEqual({
    bootSummary: {
      engineName: "null-engine",
      supportsInterrupts: true,
      supportsModuleLoader: true,
      workspaceRoot: "/workspace",
    },
    runPlan: {
      cwd: "/workspace/src",
      entrypoint: "/workspace/src/server.ts",
      envCount: 0,
      commandLine: "node server.ts",
      commandKind: "node-entrypoint",
      resolvedScript: null,
    },
    runtimeContext: expect.objectContaining({
      sessionId: session.sessionId,
      process: expect.objectContaining({
        cwd: "/workspace/src",
        commandKind: "node-entrypoint",
      }),
    }),
    engineContext: expect.objectContaining({
      sessionId: session.sessionId,
      cwd: "/workspace/src",
      entrypoint: "/workspace/src/server.ts",
      state: "booted",
      bridgeReady: false,
    }),
    bindings: expect.objectContaining({
      globals: expect.arrayContaining(["process", "Buffer", "setTimeout"]),
    }),
    bootstrapPlan: expect.objectContaining({
      bootstrapSpecifier: "runtime:bootstrap",
    }),
    startupStdout: expect.arrayContaining([
      expect.stringMatching(/^\[mount] \d+ files available at \/workspace$/),
      "[exec] node server.ts",
      expect.stringMatching(/^\[host-vfs] files=\d+ sample=.* size=\d+$/),
      "[host] engine=null-engine interrupts=true module-loader=true",
      "[plan] cwd=/workspace/src entry=/workspace/src/server.ts env=0",
      "[process] exec=/virtual/node cwd=/workspace/src argv=/virtual/node /workspace/src/server.ts",
      "[engine-context] state=booted pending-jobs=0 bridge-ready=false entry=/workspace/src/server.ts",
      "[bindings] globals=console,process,Buffer,setTimeout,clearTimeout,setInterval,clearInterval,__runtime builtins=process,fs,path,buffer,timers,console,perf_hooks,module,inspector",
      "[bootstrap] bootstrap=runtime:bootstrap modules=node:process,node:fs,node:path,node:buffer,node:timers,node:console,node:perf_hooks,node:module,node:inspector,runtime:bootstrap",
      "[context] id=runtime-context-2",
      "[detect] react=true",
      "[preview] server-ready /preview/session-1/3200/",
    ]),
    previewReady: {
      port: {
        port: 3200,
        protocol: "http",
      },
      url: "/preview/session-1/3200/",
      model: expect.objectContaining({
        title: "demo-app guest app",
      }),
      rootHydratedFiles: expect.arrayContaining([
        {
          path: "/workspace/package.json",
          size: 19,
          isText: true,
          textContent: '{"name":"demo-app"}',
          bytes: new TextEncoder().encode('{"name":"demo-app"}'),
        },
        {
          path: "/workspace/node_modules/vite/package.json",
          size: 34,
          isText: true,
          textContent: '{"bin":{"vite":"bin/vite.js"}}',
          bytes: new TextEncoder().encode('{"bin":{"vite":"bin/vite.js"}}'),
        },
      ]),
      host: {
        engineName: "null-engine",
        supportsInterrupts: true,
        supportsModuleLoader: true,
        workspaceRoot: "/workspace",
      },
      run: expect.objectContaining({
        entrypoint: "/workspace/src/server.ts",
      }),
      hostFiles: expect.objectContaining({
        count: 8,
      }),
    },
    previewLaunch: {
      startup: expect.objectContaining({
        exited: false,
        exitCode: null,
      }),
      rootReport: expect.objectContaining({
        server: {
          port: {
            port: 3200,
            protocol: "http",
          },
          kind: "preview",
          cwd: "/workspace/src",
          entrypoint: "/workspace/src/server.ts",
        },
        port: {
          port: 3200,
          protocol: "http",
        },
        request: {
          port: 3200,
          method: "GET",
          relativePath: "/",
          search: "",
          clientModules: [],
        },
        requestHint: expect.objectContaining({
          kind: "fallback-root",
        }),
        responseDescriptor: expect.objectContaining({
          kind: "host-managed-fallback",
        }),
        hydrationPaths: ["/workspace/package.json", "/workspace/node_modules/vite/package.json"],
        hydratedFiles: expect.arrayContaining([
          {
            path: "/workspace/package.json",
            size: 19,
            isText: true,
            textContent: '{"name":"demo-app"}',
            bytes: new TextEncoder().encode('{"name":"demo-app"}'),
          },
          {
            path: "/workspace/node_modules/vite/package.json",
            size: 34,
            isText: true,
            textContent: '{"bin":{"vite":"bin/vite.js"}}',
            bytes: new TextEncoder().encode('{"bin":{"vite":"bin/vite.js"}}'),
          },
        ]),
        directResponse: expect.objectContaining({
          status: 503,
          headers: expect.objectContaining({
            "content-type": "text/html; charset=utf-8",
          }),
        }),
        renderPlan: {
          kind: "host-managed-fallback",
          workspacePath: null,
          documentRoot: null,
        },
        modulePlan: null,
        transformKind: null,
      }),
    },
    state: {
      session: expect.objectContaining({
        sessionId: session.sessionId,
        state: "running",
        packageJson: expect.objectContaining({
          name: "demo-app",
        }),
        hostFiles: expect.objectContaining({
          count: 8,
        }),
      }),
      preview: expect.objectContaining({
        port: {
          port: 3200,
          protocol: "http",
        },
        rootHydratedFiles: expect.arrayContaining([
          {
            path: "/workspace/package.json",
            size: 19,
            isText: true,
            textContent: '{"name":"demo-app"}',
            bytes: new TextEncoder().encode('{"name":"demo-app"}'),
          },
          {
            path: "/workspace/node_modules/vite/package.json",
            size: 34,
            isText: true,
            textContent: '{"bin":{"vite":"bin/vite.js"}}',
            bytes: new TextEncoder().encode('{"bin":{"vite":"bin/vite.js"}}'),
          },
        ]),
        run: expect.objectContaining({
          entrypoint: "/workspace/src/server.ts",
        }),
        hostFiles: expect.objectContaining({
          count: 8,
        }),
        rootRequestHint: expect.objectContaining({
          kind: "fallback-root",
        }),
        rootResponseDescriptor: expect.objectContaining({
          kind: "host-managed-fallback",
        }),
      }),
    },
    events: [
      {
        kind: "port-listen",
        port: {
          port: 3200,
          protocol: "http",
        },
      },
    ],
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.run-until-idle",
      maxTurns: 16,
    }),
  ).resolves.toEqual({
    kind: "runtime-idle-report",
    report: {
      turns: 0,
      drainedJobs: 0,
      firedTimers: 0,
      nowMs: 0,
      pendingJobs: 0,
      pendingTimers: 0,
      exited: false,
      exitCode: null,
      reachedTurnLimit: false,
    },
  });

  await expect(adapter.describeEngineContext(runtimeContext.contextId)).resolves.toEqual({
    engineSessionId: `null-engine-session:${runtimeContext.sessionId}`,
    engineContextId: `null-engine-context:${runtimeContext.contextId}`,
    sessionId: runtimeContext.sessionId,
    cwd: "/workspace/src",
    entrypoint: "/workspace/src/server.ts",
    argvLen: 2,
    envCount: 0,
    pendingJobs: 0,
    registeredModules: 10,
    bootstrapSpecifier: "runtime:bootstrap",
    bridgeReady: true,
    moduleLoaderRoots: ["/workspace/node_modules", "/workspace/src/node_modules"],
    state: "ready",
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.describe-modules",
    }),
  ).resolves.toEqual({
    kind: "runtime-module-list",
    modules: expect.arrayContaining([
      {
        specifier: "node:process",
        sourceLen: expect.any(Number),
      },
      {
        specifier: "runtime:bootstrap",
        sourceLen: expect.any(Number),
      },
    ]),
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.read-module",
      specifier: "runtime:bootstrap",
    }),
  ).resolves.toEqual({
    kind: "runtime-module-source",
    module: expect.objectContaining({
      specifier: "runtime:bootstrap",
      source: expect.stringContaining('import("/workspace/src/server.ts")'),
    }),
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.resolve-module",
      importer: "/workspace/src/server.ts",
      specifier: "./boot",
    }),
  ).resolves.toEqual({
    kind: "runtime-module-resolved",
    module: {
      requestedSpecifier: "./boot",
      resolvedSpecifier: "/workspace/src/boot.ts",
      kind: "workspace",
      format: "module",
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.prepare-module-import",
      importer: "/workspace/src/server.ts",
      specifier: "./boot",
    }),
  ).resolves.toEqual({
    kind: "runtime-module-import-plan",
    plan: {
      requestSpecifier: "./boot",
      importer: "/workspace/src/server.ts",
      resolvedModule: {
        requestedSpecifier: "./boot",
        resolvedSpecifier: "/workspace/src/boot.ts",
        kind: "workspace",
        format: "module",
      },
      loadedModule: {
        resolvedSpecifier: "/workspace/src/boot.ts",
        kind: "workspace",
        format: "module",
        source: "export const boot = true;",
      },
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.load-module",
      resolvedSpecifier: "/workspace/src/boot.ts",
    }),
  ).resolves.toEqual({
    kind: "runtime-module-loaded",
    module: {
      resolvedSpecifier: "/workspace/src/boot.ts",
      kind: "workspace",
      format: "module",
      source: "export const boot = true;",
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
    queueLen: 2,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "console.emit",
      level: "warn",
      values: ["watch", "out"],
    }),
  ).resolves.toEqual({
    kind: "event-queued",
    queueLen: 4,
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
          port: 3100,
          protocol: "http",
        },
      },
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
        port: 3100,
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
          port: 3100,
          protocol: "http",
        },
        kind: "preview",
        cwd: "/workspace/src",
        entrypoint: "/workspace/src/server.ts",
      },
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
        clientModules: [],
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
      clientModules: [],
    },
    requestHint: expect.objectContaining({
      kind: "workspace-asset",
      workspacePath: "/workspace/src/server.ts",
      documentRoot: "/workspace",
      hydratePaths: [
        "/workspace/package.json",
        "/workspace/node_modules/vite/package.json",
        "/workspace/src/server.ts",
      ],
    }),
    responseDescriptor: expect.objectContaining({
      kind: "workspace-asset",
      workspacePath: "/workspace/src/server.ts",
      documentRoot: "/workspace",
      hydratePaths: [
        "/workspace/package.json",
        "/workspace/node_modules/vite/package.json",
        "/workspace/src/server.ts",
      ],
      statusCode: 200,
      contentType: "text/plain; charset=utf-8",
      allowMethods: [],
      omitBody: false,
    }),
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.preview-request",
      request: {
        port: 4200,
        method: "GET",
        relativePath: "/src/server.ts",
        search: "?v=1",
        clientModules: [],
      },
    }),
  ).resolves.toEqual({
    kind: "runtime-preview-request",
    report: {
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
        clientModules: [],
      },
      requestHint: expect.objectContaining({
        kind: "workspace-asset",
        workspacePath: "/workspace/src/server.ts",
        documentRoot: "/workspace",
        hydratePaths: [
          "/workspace/package.json",
          "/workspace/node_modules/vite/package.json",
          "/workspace/src/server.ts",
        ],
      }),
      responseDescriptor: expect.objectContaining({
        kind: "workspace-asset",
        workspacePath: "/workspace/src/server.ts",
        documentRoot: "/workspace",
        hydratePaths: [
          "/workspace/package.json",
          "/workspace/node_modules/vite/package.json",
          "/workspace/src/server.ts",
        ],
        statusCode: 200,
        contentType: "text/plain; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      }),
      hydrationPaths: [
        "/workspace/package.json",
        "/workspace/node_modules/vite/package.json",
        "/workspace/src/server.ts",
      ],
      transformKind: "module",
      renderPlan: {
        kind: "workspace-file",
        workspacePath: "/workspace/src/server.ts",
        documentRoot: "/workspace",
      },
      modulePlan: {
        importerPath: "/workspace/src/server.ts",
        format: "module",
        importPlans: [],
      },
      directResponse: null,
      hydratedFiles: [
        {
          path: "/workspace/package.json",
          size: 19,
          isText: true,
          textContent: '{"name":"demo-app"}',
          bytes: new TextEncoder().encode('{"name":"demo-app"}'),
        },
        {
          path: "/workspace/node_modules/vite/package.json",
          size: 34,
          isText: true,
          textContent: '{"bin":{"vite":"bin/vite.js"}}',
          bytes: new TextEncoder().encode('{"bin":{"vite":"bin/vite.js"}}'),
        },
        {
          path: "/workspace/src/server.ts",
          size: 20,
          isText: true,
          textContent: "console.log('server')",
          bytes: new TextEncoder().encode("console.log('server')"),
        },
      ],
    },
  });

  await expect(
    adapter.writeWorkspaceFile(session.sessionId, {
      path: "/workspace/src/app.css",
      bytes: new TextEncoder().encode('body{background:url("/bg.png")}'),
      isText: true,
    }),
  ).resolves.toEqual({
    path: "/workspace/src/app.css",
    kind: "file",
    size: 31,
    isText: true,
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.preview-request",
      request: {
        port: 4200,
        method: "GET",
        relativePath: "/files/src/app.css",
        search: "",
        clientModules: [],
      },
    }),
  ).resolves.toEqual({
    kind: "runtime-preview-request",
    report: {
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
        relativePath: "/files/src/app.css",
        search: "",
        clientModules: [],
      },
      requestHint: expect.objectContaining({
        kind: "workspace-file",
        workspacePath: "/workspace/src/app.css",
        documentRoot: "/workspace",
        hydratePaths: [
          "/workspace/package.json",
          "/workspace/node_modules/vite/package.json",
          "/workspace/src/app.css",
        ],
      }),
      responseDescriptor: expect.objectContaining({
        kind: "workspace-file",
        workspacePath: "/workspace/src/app.css",
        documentRoot: "/workspace",
        hydratePaths: [
          "/workspace/package.json",
          "/workspace/node_modules/vite/package.json",
          "/workspace/src/app.css",
        ],
        statusCode: 200,
        contentType: "text/css; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      }),
      hydrationPaths: [
        "/workspace/package.json",
        "/workspace/node_modules/vite/package.json",
        "/workspace/src/app.css",
      ],
      transformKind: "stylesheet",
      renderPlan: {
        kind: "workspace-file",
        workspacePath: "/workspace/src/app.css",
        documentRoot: "/workspace",
      },
      modulePlan: null,
      directResponse: {
        status: 200,
        headers: {
          "content-type": "text/css; charset=utf-8",
          "cache-control": "no-store",
        },
        textBody: 'body{background:url("/preview/session-1/4200/bg.png")}',
        bytes: null,
      },
      hydratedFiles: [
        {
          path: "/workspace/package.json",
          size: 19,
          isText: true,
          textContent: '{"name":"demo-app"}',
          bytes: new TextEncoder().encode('{"name":"demo-app"}'),
        },
        {
          path: "/workspace/node_modules/vite/package.json",
          size: 34,
          isText: true,
          textContent: '{"bin":{"vite":"bin/vite.js"}}',
          bytes: new TextEncoder().encode('{"bin":{"vite":"bin/vite.js"}}'),
        },
        {
          path: "/workspace/src/app.css",
          size: 31,
          isText: true,
          textContent: 'body{background:url("/bg.png")}',
          bytes: new TextEncoder().encode('body{background:url("/bg.png")}'),
        },
      ],
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.preview-request",
      request: {
        port: 4200,
        method: "GET",
        relativePath: "/files/logo.png",
        search: "",
        clientModules: [],
      },
    }),
  ).resolves.toEqual({
    kind: "runtime-preview-request",
    report: {
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
        relativePath: "/files/logo.png",
        search: "",
        clientModules: [],
      },
      requestHint: expect.objectContaining({
        kind: "workspace-file",
        workspacePath: "/workspace/logo.png",
        documentRoot: "/workspace",
        hydratePaths: [
          "/workspace/package.json",
          "/workspace/node_modules/vite/package.json",
          "/workspace/logo.png",
        ],
      }),
      responseDescriptor: expect.objectContaining({
        kind: "workspace-file",
        workspacePath: "/workspace/logo.png",
        documentRoot: "/workspace",
        hydratePaths: [
          "/workspace/package.json",
          "/workspace/node_modules/vite/package.json",
          "/workspace/logo.png",
        ],
        statusCode: 200,
        contentType: "image/png",
        allowMethods: [],
        omitBody: false,
      }),
      hydrationPaths: [
        "/workspace/package.json",
        "/workspace/node_modules/vite/package.json",
        "/workspace/logo.png",
      ],
      transformKind: "binary",
      renderPlan: {
        kind: "workspace-file",
        workspacePath: "/workspace/logo.png",
        documentRoot: "/workspace",
      },
      modulePlan: null,
      directResponse: {
        status: 200,
        headers: {
          "content-type": "image/png",
          "cache-control": "no-store",
        },
        textBody: null,
        bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47]),
      },
      hydratedFiles: [
        {
          path: "/workspace/package.json",
          size: 19,
          isText: true,
          textContent: '{"name":"demo-app"}',
          bytes: new TextEncoder().encode('{"name":"demo-app"}'),
        },
        {
          path: "/workspace/node_modules/vite/package.json",
          size: 34,
          isText: true,
          textContent: '{"bin":{"vite":"bin/vite.js"}}',
          bytes: new TextEncoder().encode('{"bin":{"vite":"bin/vite.js"}}'),
        },
        {
          path: "/workspace/logo.png",
          size: 4,
          isText: false,
          textContent: null,
          bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47]),
        },
      ],
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.preview-request",
      request: {
        port: 4200,
        method: "GET",
        relativePath: "/",
        search: "",
        clientModules: [{ specifier: "runtime:preview-client", url: "/assets/preview-client.js" }],
      },
    }),
  ).resolves.toMatchObject({
    kind: "runtime-preview-request",
    report: {
      requestHint: {
        kind: "fallback-root",
      },
      responseDescriptor: {
        kind: "host-managed-fallback",
      },
      directResponse: {
        status: 200,
        headers: {
          "content-type": "text/html; charset=utf-8",
          "cache-control": "no-store",
        },
        textBody: expect.stringContaining("/assets/preview-client.js"),
        bytes: null,
      },
    },
  });

  const sample3Session: SessionSnapshot = {
    sessionId: "sample3-session",
    state: "mounted",
    revision: 0,
    workspaceRoot: "/workspace",
    archive: {
      fileName: "sample3.zip",
      fileCount: 3,
      directoryCount: 2,
      entries: [],
      rootPrefix: null,
    },
    packageJson: {
      name: "sample3-app",
      scripts: {
        dev: "react-router dev",
      },
      dependencies: ["react", "react-dom"],
      devDependencies: [],
    },
    capabilities: {
      detectedReact: true,
    },
  };
  const sample3Files = new Map<string, WorkspaceFileRecord>([
    [
      "/workspace/package.json",
      {
        path: "/workspace/package.json",
        size: 63,
        contentType: "application/json; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode(
          '{"name":"sample3-app","dependencies":["react","react-dom"]}',
        ),
        textContent: '{"name":"sample3-app","dependencies":["react","react-dom"]}',
      },
    ],
    [
      "/workspace/app/routes/home.tsx",
      {
        path: "/workspace/app/routes/home.tsx",
        size: 67,
        contentType: "text/plain; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode(
          "export default function Home() { return <section>sample3</section>; }",
        ),
        textContent: "export default function Home() { return <section>sample3</section>; }",
      },
    ],
    [
      "/workspace/app/app.css",
      {
        path: "/workspace/app/app.css",
        size: 18,
        contentType: "text/css; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode("body { margin: 0; }"),
        textContent: "body { margin: 0; }",
      },
    ],
    [
      "/workspace/node_modules/react/package.json",
      {
        path: "/workspace/node_modules/react/package.json",
        size: 18,
        contentType: "application/json; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode('{"main":"index.js"}'),
        textContent: '{"main":"index.js"}',
      },
    ],
    [
      "/workspace/node_modules/react/index.js",
      {
        path: "/workspace/node_modules/react/index.js",
        size: 33,
        contentType: "text/plain; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode("export const createElement = () => null;"),
        textContent: "export const createElement = () => null;",
      },
    ],
    [
      "/workspace/node_modules/react-dom/client.js",
      {
        path: "/workspace/node_modules/react-dom/client.js",
        size: 58,
        contentType: "text/plain; charset=utf-8",
        isText: true,
        bytes: new TextEncoder().encode("export function createRoot() { return { render() {} }; }"),
        textContent: "export function createRoot() { return { render() {} }; }",
      },
    ],
  ]);
  await adapter.createSession({
    sessionId: sample3Session.sessionId,
    session: sample3Session,
    files: sample3Files,
  });
  const sample3RuntimeContext = await adapter.createRuntimeContext(sample3Session.sessionId, {
    cwd: "/workspace",
    command: "npm",
    args: ["run", "dev"],
  });
  await adapter.executeRuntimeCommand(sample3RuntimeContext.contextId, {
    kind: "http.serve-preview",
    port: 4300,
  });
  await expect(
    adapter.executeRuntimeCommand(sample3RuntimeContext.contextId, {
      kind: "runtime.preview-request",
      request: {
        port: 4300,
        method: "GET",
        relativePath: "/",
        search: "",
        clientModules: [{ specifier: "runtime:preview-client", url: "/assets/preview-client.js" }],
      },
    }),
  ).resolves.toMatchObject({
    kind: "runtime-preview-request",
    report: {
      requestHint: {
        kind: "root-entry",
        workspacePath: "/workspace/app/routes/home.tsx",
      },
      responseDescriptor: {
        kind: "app-shell",
        workspacePath: "/workspace/app/routes/home.tsx",
      },
      directResponse: {
        status: 200,
        headers: {
          "content-type": "text/html; charset=utf-8",
          "cache-control": "no-store",
        },
        textBody: expect.stringContaining("globalThis.process ??="),
        bytes: null,
      },
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.preview-request",
      request: {
        port: 4200,
        method: "GET",
        relativePath: "/__bootstrap.json",
        search: "",
        clientModules: [],
      },
    }),
  ).resolves.toMatchObject({
    kind: "runtime-preview-request",
    report: {
      requestHint: {
        kind: "bootstrap-state",
      },
      transformKind: null,
      renderPlan: null,
      responseDescriptor: {
        kind: "bootstrap-state",
      },
      directResponse: {
        status: 200,
        headers: {
          "content-type": "application/json; charset=utf-8",
          "cache-control": "no-store",
        },
        textBody: expect.stringContaining('"preview":{"type":"preview.ready"'),
        bytes: null,
      },
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.preview-request",
      request: {
        port: 4200,
        method: "GET",
        relativePath: "/__workspace.json",
        search: "",
        clientModules: [],
      },
    }),
  ).resolves.toMatchObject({
    kind: "runtime-preview-request",
    report: {
      requestHint: {
        kind: "workspace-state",
      },
      transformKind: null,
      renderPlan: null,
      responseDescriptor: {
        kind: "workspace-state",
      },
      directResponse: {
        status: 200,
        headers: {
          "content-type": "application/json; charset=utf-8",
          "cache-control": "no-store",
        },
        textBody: expect.stringContaining('"sessionId":"session-1"'),
        bytes: null,
      },
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.preview-request",
      request: {
        port: 4200,
        method: "GET",
        relativePath: "/assets/runtime.css",
        search: "",
        clientModules: [],
      },
    }),
  ).resolves.toMatchObject({
    kind: "runtime-preview-request",
    report: {
      requestHint: {
        kind: "runtime-stylesheet",
      },
      transformKind: null,
      renderPlan: null,
      responseDescriptor: {
        kind: "runtime-stylesheet",
      },
      directResponse: {
        status: 200,
        headers: {
          "content-type": "text/css; charset=utf-8",
          "cache-control": "no-store",
        },
        textBody: expect.stringContaining(".guest-shell"),
        bytes: null,
      },
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "http.resolve-preview",
      request: {
        port: 4200,
        method: "HEAD",
        relativePath: "/src/server.ts",
        search: "",
        clientModules: [],
      },
    }),
  ).resolves.toMatchObject({
    kind: "preview-request-resolved",
    responseDescriptor: {
      kind: "workspace-asset",
      hydratePaths: [],
      statusCode: 200,
      contentType: "text/plain; charset=utf-8",
      allowMethods: [],
      omitBody: true,
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "http.resolve-preview",
      request: {
        port: 4200,
        method: "POST",
        relativePath: "/src/server.ts",
        search: "",
        clientModules: [],
      },
    }),
  ).resolves.toMatchObject({
    kind: "preview-request-resolved",
    responseDescriptor: {
      kind: "method-not-allowed",
      hydratePaths: [],
      statusCode: 405,
      contentType: "application/json; charset=utf-8",
      allowMethods: ["GET", "HEAD"],
      omitBody: false,
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
      kind: "fs.write-file",
      path: "generated/output.json",
      bytes: new TextEncoder().encode('{"ok":true}'),
      isText: true,
    }),
  ).resolves.toEqual({
    kind: "fs",
    response: {
      kind: "entry",
      entry: {
        path: "/workspace/src/generated/output.json",
        kind: "file",
        size: 11,
        isText: true,
      },
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.drain-events",
    }),
  ).resolves.toEqual({
    kind: "runtime-events",
    events: [
      expect.objectContaining({
        kind: "workspace-change",
        entry: {
          path: "/workspace/src/generated/output.json",
          kind: "file",
          size: 11,
          isText: true,
        },
        revision: 1,
        state: expect.objectContaining({
          session: expect.objectContaining({
            revision: 1,
          }),
        }),
      }),
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
      kind: "runtime.shutdown",
      code: 0,
    }),
  ).resolves.toEqual({
    kind: "runtime-shutdown",
    report: {
      contextId: runtimeContext.contextId,
      sessionId: session.sessionId,
      exitCode: 0,
      closedPorts: [
        {
          port: 3100,
          protocol: "http",
        },
        {
          port: 4100,
          protocol: "http",
        },
      ],
      closedServers: [
        {
          port: {
            port: 3100,
            protocol: "http",
          },
          kind: "preview",
          cwd: "/workspace/src",
          entrypoint: "/workspace/src/server.ts",
        },
      ],
      events: [
        {
          kind: "workspace-change",
          entry: {
            path: "/workspace/src/generated/nested/context.log",
            kind: "file",
            size: 13,
            isText: true,
          },
          revision: 2,
          state: expect.objectContaining({
            session: expect.objectContaining({
              revision: 2,
            }),
          }),
        },
        {
          kind: "process-exit",
          code: 0,
        },
        {
          kind: "port-close",
          port: 3100,
        },
        {
          kind: "port-close",
          port: 4100,
        },
      ],
    },
  });

  await expect(
    adapter.executeRuntimeCommand(runtimeContext.contextId, {
      kind: "runtime.drain-events",
    }),
  ).rejects.toThrow("runtime context not found");

  await expect(
    adapter.launchRuntime(
      session.sessionId,
      {
        cwd: "/workspace",
        command: "npm",
        args: ["run", "dev"],
      },
      {
        maxTurns: 16,
        port: 3300,
      },
    ),
  ).resolves.toEqual(
    expect.objectContaining({
      runPlan: expect.objectContaining({
        cwd: "/workspace",
        commandKind: "npm-script",
        commandLine: "npm run dev",
        resolvedScript: "vite",
      }),
      startupStdout: expect.arrayContaining([
        "[browser-cli] runtime=browser-dev-server preview=http-server mode=dev",
      ]),
      previewReady: expect.objectContaining({
        url: "/preview/session-1/3300/",
      }),
    }),
  );

  await expect(
    adapter.executeContextFsCommand(runtimeContext.contextId, {
      kind: "read-file",
      path: "server.ts",
    }),
  ).rejects.toThrow("runtime context not found");

  await expect(adapter.dropRuntimeContext(runtimeContext.contextId)).rejects.toThrow(
    "runtime context not found",
  );
});
