import ReactDOM from "react-dom/client";

import { App } from "./App";
import { withAppBasePath } from "./runtime/app-base";
import "./style.css";

type RuntimeHostSmokeWindow = Window & {
  __runtimeHostSmoke?: {
    bootSummary(wasmUrl?: string): Promise<{
      engineName: string;
      supportsInterrupts: boolean;
      supportsModuleLoader: boolean;
      workspaceRoot: string;
    }>;
    launchRuntime(wasmUrl?: string): Promise<{
      bootSummary: {
        engineName: string;
        supportsInterrupts: boolean;
        supportsModuleLoader: boolean;
        workspaceRoot: string;
      };
      engineContext: {
        state: string;
        bridgeReady: boolean;
        bootstrapSpecifier: string | null;
        registeredModules: number;
      };
      startupStdout: string[];
      previewReadyUrl: string | null;
    }>;
  };
};

const container = document.querySelector<HTMLDivElement>("#app");

if (!container) {
  throw new Error("Missing #app root");
}

const runtimeHostSmokeWindow = window as RuntimeHostSmokeWindow;
runtimeHostSmokeWindow.__runtimeHostSmoke = {
  async bootSummary(wasmUrl = withAppBasePath("/runtime-host-qjs.wasm")) {
    const { WasmRuntimeHostAdapter } = await import("./runtime/host-adapter");
    const adapter = await WasmRuntimeHostAdapter.create(wasmUrl);
    return await adapter.bootSummary();
  },
  async launchRuntime(wasmUrl = withAppBasePath("/runtime-host-qjs.wasm")) {
    const { WasmRuntimeHostAdapter } = await import("./runtime/host-adapter");

    const adapter = await WasmRuntimeHostAdapter.create(wasmUrl);
    const sessionId = "browser-smoke-session";
    const workspaceRoot = "/workspace";
    const encoder = new TextEncoder();
    const files = new Map([
      [
        "/workspace/package.json",
        {
          path: "/workspace/package.json",
          size: 28,
          contentType: "application/json; charset=utf-8",
          isText: true,
          bytes: encoder.encode('{"name":"browser-smoke-app"}'),
          textContent: '{"name":"browser-smoke-app"}',
        },
      ],
      [
        "/workspace/src/server.js",
        {
          path: "/workspace/src/server.js",
          size: 21,
          contentType: "text/javascript; charset=utf-8",
          isText: true,
          bytes: encoder.encode("console.log('browser');"),
          textContent: "console.log('browser');",
        },
      ],
    ]);

    await adapter.createSession({
      sessionId,
      session: {
        sessionId,
        state: "mounted",
        revision: 0,
        workspaceRoot,
        archive: {
          fileName: "browser-smoke.zip",
          fileCount: files.size,
          directoryCount: 2,
          entries: [],
          rootPrefix: null,
        },
        packageJson: {
          name: "browser-smoke-app",
          scripts: {},
          dependencies: [],
          devDependencies: [],
        },
        capabilities: {
          detectedReact: false,
        },
      },
      files,
    });

    const report = await adapter.launchRuntime(
      sessionId,
      {
        cwd: workspaceRoot,
        command: "node",
        args: ["/workspace/src/server.js"],
      },
      {
        maxTurns: 8,
      },
    );

    return {
      bootSummary: report.bootSummary,
      engineContext: {
        state: report.engineContext.state,
        bridgeReady: report.engineContext.bridgeReady,
        bootstrapSpecifier: report.engineContext.bootstrapSpecifier,
        registeredModules: report.engineContext.registeredModules,
      },
      startupStdout: report.startupStdout,
      previewReadyUrl: report.previewReady?.url ?? null,
    };
  },
};

ReactDOM.createRoot(container).render(<App />);
