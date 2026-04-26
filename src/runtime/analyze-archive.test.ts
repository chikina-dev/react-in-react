import { strToU8, zipSync } from "fflate";
import { expect, test } from "vite-plus/test";

import {
  detectCommonPrefix,
  guessContentType,
  mountArchive,
  normalizeArchivePath,
  parseArchive,
} from "./analyze-archive";

test("detectCommonPrefix strips the single top-level workspace folder", () => {
  const prefix = detectCommonPrefix([
    "demo/package.json",
    "demo/src/main.tsx",
    "demo/node_modules/react/package.json",
  ]);

  expect(prefix).toEqual({
    prefix: "demo",
    segments: ["demo"],
  });
});

test("normalizeArchivePath maps entries into /workspace", () => {
  const prefix = detectCommonPrefix(["demo/package.json", "demo/src/main.tsx"]);

  expect(normalizeArchivePath("demo/src/main.tsx", prefix)).toBe("/workspace/src/main.tsx");
});

test("parseArchive extracts package.json and react capability", () => {
  const archive = zipSync({
    "demo/package.json": strToU8(
      JSON.stringify({
        name: "guest-react-app",
        scripts: { dev: "vite" },
        dependencies: {
          react: "^19.0.0",
          "react-dom": "^19.0.0",
        },
      }),
    ),
    "demo/src/main.tsx": strToU8("console.log('hello');"),
    "demo/node_modules/react/package.json": strToU8('{"name":"react"}'),
  });

  const parsed = parseArchive("guest.zip", archive);

  expect(parsed.packageJson?.name).toBe("guest-react-app");
  expect(parsed.packageJson?.scripts.dev).toBe("vite");
  expect(parsed.archive.entries[0]?.path.startsWith("/workspace")).toBe(true);
  expect(parsed.capabilities.detectedReact).toBe(true);
});

test("mountArchive derives suggested run request with dev-first policy", () => {
  const archive = zipSync({
    "demo/package.json": strToU8(
      JSON.stringify({
        name: "guest-app",
        scripts: {
          start: "node server.js",
          dev: "vite",
        },
      }),
    ),
  });

  const zipBytes = new Uint8Array(archive);
  const mounted = mountArchive("guest.zip", zipBytes.buffer as ArrayBuffer, "session-1");

  expect(mounted.snapshot.suggestedRunRequest).toEqual({
    cwd: "/workspace",
    command: "npm",
    args: ["run", "dev"],
  });
});

test("guessContentType treats node_modules .bin wrappers as text", () => {
  expect(
    guessContentType(
      "/workspace/node_modules/.bin/vite",
      strToU8("#!/usr/bin/env node\nconsole.log('vite');\n"),
    ),
  ).toBe("text/plain; charset=utf-8");
});
