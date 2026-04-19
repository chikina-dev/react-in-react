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
});
