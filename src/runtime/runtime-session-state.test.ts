import { expect, test } from "vite-plus/test";

import type { SessionSnapshot } from "./protocol";
import {
  applyPackageJsonTextToSessionSnapshot,
  deriveSuggestedRunRequest,
  deriveSuggestedRunScriptName,
  applyWorkspaceEntryToSessionSnapshot,
  parsePackageJsonSummary,
} from "./runtime-session-state";

function createSession(): SessionSnapshot {
  return {
    sessionId: "session-1",
    state: "mounted",
    revision: 0,
    workspaceRoot: "/workspace",
    archive: {
      fileName: "demo.zip",
      fileCount: 1,
      directoryCount: 1,
      entries: [
        { path: "/workspace", size: 0, kind: "dir" },
        { path: "/workspace/src/main.tsx", size: 12, kind: "file" },
      ],
      rootPrefix: "demo",
    },
    packageJson: null,
    capabilities: {
      detectedReact: false,
    },
  };
}

test("applyWorkspaceEntryToSessionSnapshot upserts file entries and counts", () => {
  const session = createSession();

  applyWorkspaceEntryToSessionSnapshot(session, {
    path: "/workspace/src/generated/app.js",
    kind: "file",
    size: 24,
    isText: true,
  });

  expect(session.archive.fileCount).toBe(2);
  expect(session.archive.directoryCount).toBe(1);
  expect(session.archive.entries).toContainEqual({
    path: "/workspace/src/generated/app.js",
    size: 24,
    kind: "file",
  });
});

test("applyWorkspaceEntryToSessionSnapshot upserts directory entries and counts", () => {
  const session = createSession();

  applyWorkspaceEntryToSessionSnapshot(session, {
    path: "/workspace/src/generated",
    kind: "directory",
    size: 0,
    isText: false,
  });

  expect(session.archive.fileCount).toBe(1);
  expect(session.archive.directoryCount).toBe(2);
  expect(session.archive.entries).toContainEqual({
    path: "/workspace/src/generated",
    size: 0,
    kind: "dir",
  });
});

test("applyPackageJsonTextToSessionSnapshot updates package summary and capability matrix", () => {
  const session = createSession();

  applyPackageJsonTextToSessionSnapshot(
    session,
    JSON.stringify({
      name: "demo-app",
      scripts: { dev: "vite" },
      dependencies: { react: "^19.0.0", "react-dom": "^19.0.0" },
      devDependencies: { vite: "^8.0.0" },
    }),
  );

  expect(session.packageJson).toEqual({
    name: "demo-app",
    scripts: { dev: "vite" },
    dependencies: ["react", "react-dom"],
    devDependencies: ["vite"],
  });
  expect(session.suggestedRunRequest).toEqual({
    cwd: "/workspace",
    command: "npm",
    args: ["run", "dev"],
  });
  expect(session.capabilities).toEqual({
    detectedReact: true,
  });
});

test("parsePackageJsonSummary returns null for invalid JSON", () => {
  expect(parsePackageJsonSummary("{")).toBeNull();
});

test("deriveSuggestedRunRequest prefers dev before start", () => {
  expect(
    deriveSuggestedRunRequest(
      {
        name: "demo-app",
        scripts: {
          dev: "vite",
          start: "node server.js",
        },
        dependencies: [],
        devDependencies: [],
      },
      "/workspace",
    ),
  ).toEqual({
    cwd: "/workspace",
    command: "npm",
    args: ["run", "dev"],
  });
  expect(
    deriveSuggestedRunScriptName({
      name: "demo-app",
      scripts: {
        dev: "vite",
        start: "node server.js",
      },
      dependencies: [],
      devDependencies: [],
    }),
  ).toBe("dev");
});

test("deriveSuggestedRunRequest falls back to start when dev is absent", () => {
  expect(
    deriveSuggestedRunRequest(
      {
        name: "demo-app",
        scripts: {
          start: "node server.js",
        },
        dependencies: [],
        devDependencies: [],
      },
      "/workspace",
    ),
  ).toEqual({
    cwd: "/workspace",
    command: "npm",
    args: ["run", "start"],
  });
  expect(
    deriveSuggestedRunScriptName({
      name: "demo-app",
      scripts: {
        start: "node server.js",
      },
      dependencies: [],
      devDependencies: [],
    }),
  ).toBe("start");
});
