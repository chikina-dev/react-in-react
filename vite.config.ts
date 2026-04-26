// @ts-nocheck
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vite-plus";

const projectRoot = dirname(fileURLToPath(import.meta.url));
const base = normalizeBasePath(process.env.VITE_BASE_PATH ?? "/");
const runtimeHostWasmSource = resolve(
  projectRoot,
  "target/wasm32-unknown-unknown/debug/runtime_host.wasm",
);
const runtimeHostWasmPublic = resolve(projectRoot, "public/runtime-host.wasm");
const runtimeHostQuickJsWasmPublic = resolve(projectRoot, "public/runtime-host-qjs.wasm");

function syncRustRuntimeHostWasm(options?: {
  outputPath?: string;
  quickjsNgEngine?: boolean;
}): void {
  const outputPath = options?.outputPath ?? runtimeHostWasmPublic;
  const quickjsNgEngine = options?.quickjsNgEngine ?? false;
  const args = ["build", "--target", "wasm32-unknown-unknown", "-p", "runtime-host"];

  if (quickjsNgEngine) {
    args.push("--features", "quickjs-ng-engine");
  }

  const build = spawnSync("cargo", args, {
    cwd: projectRoot,
    stdio: "pipe",
    encoding: "utf-8",
  });

  if (build.status !== 0) {
    throw new Error(build.stderr || build.stdout || "Failed to build runtime-host wasm.");
  }

  if (!existsSync(runtimeHostWasmSource)) {
    throw new Error(`runtime-host wasm output was not found at ${runtimeHostWasmSource}`);
  }

  mkdirSync(dirname(outputPath), { recursive: true });
  copyFileSync(runtimeHostWasmSource, outputPath);
}

function normalizeBasePath(input: string): string {
  return input.endsWith("/") ? input : `${input}/`;
}

export default defineConfig({
  base,
  test: {
    include: ["src/**/*.test.ts", "src/**/*.test.tsx", "src/**/*.spec.ts", "src/**/*.spec.tsx"],
    exclude: ["sample/**", "e2e/**"],
  },
  plugins: [
    {
      name: "runtime-host-wasm",
      buildStart() {
        syncRustRuntimeHostWasm();
        syncRustRuntimeHostWasm({
          outputPath: runtimeHostQuickJsWasmPublic,
          quickjsNgEngine: true,
        });
      },
      configureServer() {
        syncRustRuntimeHostWasm();
        syncRustRuntimeHostWasm({
          outputPath: runtimeHostQuickJsWasmPublic,
          quickjsNgEngine: true,
        });
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
