import { expect, test } from "vite-plus/test";

import {
  isPreviewPathForBase,
  normalizeBasePath,
  normalizePreviewTextForBase,
  stripBasePath,
  withBasePath,
} from "./app-base";

test("normalizes GitHub Pages base paths", () => {
  expect(normalizeBasePath("/node-in-node")).toBe("/node-in-node/");
  expect(normalizeBasePath("/node-in-node/")).toBe("/node-in-node/");
});

test("prefixes preview URLs with a project-pages base path", () => {
  expect(withBasePath("/preview/session-1/3000/", "/node-in-node/")).toBe(
    "/node-in-node/preview/session-1/3000/",
  );
  expect(withBasePath("/runtime-host-qjs.wasm", "/node-in-node/")).toBe(
    "/node-in-node/runtime-host-qjs.wasm",
  );
});

test("strips a project-pages base path before preview routing", () => {
  expect(stripBasePath("/node-in-node/preview/session-1/3000/src/main.tsx", "/node-in-node/")).toBe(
    "/preview/session-1/3000/src/main.tsx",
  );
  expect(isPreviewPathForBase("/node-in-node/preview/session-1/3000/", "/node-in-node/")).toBe(
    true,
  );
});

test("rewrites preview links inside backend-owned text payloads", () => {
  expect(
    normalizePreviewTextForBase(
      'body{background:url("/preview/session-1/3000/bg.png")}',
      "/node-in-node/",
    ),
  ).toBe('body{background:url("/node-in-node/preview/session-1/3000/bg.png")}');
});
