// @ts-nocheck
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vite-plus";

const projectRoot = dirname(fileURLToPath(import.meta.url));
const runtimeHostWasmSource = resolve(
  projectRoot,
  "target/wasm32-unknown-unknown/debug/runtime_host.wasm",
);
const runtimeHostWasmPublic = resolve(projectRoot, "public/runtime-host.wasm");

function syncRustRuntimeHostWasm(): void {
  const build = spawnSync(
    "cargo",
    ["build", "--target", "wasm32-unknown-unknown", "-p", "runtime-host"],
    {
      cwd: projectRoot,
      stdio: "pipe",
      encoding: "utf-8",
    },
  );

  if (build.status !== 0) {
    throw new Error(build.stderr || build.stdout || "Failed to build runtime-host wasm.");
  }

  if (!existsSync(runtimeHostWasmSource)) {
    throw new Error(`runtime-host wasm output was not found at ${runtimeHostWasmSource}`);
  }

  mkdirSync(dirname(runtimeHostWasmPublic), { recursive: true });
  copyFileSync(runtimeHostWasmSource, runtimeHostWasmPublic);
}

export default defineConfig({
  test: {
    include: ["src/**/*.test.ts", "src/**/*.test.tsx", "src/**/*.spec.ts", "src/**/*.spec.tsx"],
    exclude: ["sample/**", "e2e/**"],
  },
  plugins: [
    {
      name: "runtime-host-wasm",
      buildStart() {
        syncRustRuntimeHostWasm();
      },
      configureServer() {
        syncRustRuntimeHostWasm();
      },
    },
  ],
  staged: {
    "*": "vp check --fix",
  },
  fmt: {},
  lint: {
    ignorePatterns: ["sample/**"],
    options: { typeAware: true, typeCheck: true },
  },
});
