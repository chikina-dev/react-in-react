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
});
