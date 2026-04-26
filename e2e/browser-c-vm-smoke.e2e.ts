import { expect, test } from "@playwright/test";

test("instantiates the feature-enabled browser C VM wasm and reports its boot summary", async ({
  page,
}) => {
  await page.goto("/");

  const summary = await page.evaluate(async () => {
    const runtimeHostSmoke = (
      window as Window & {
        __runtimeHostSmoke?: {
          bootSummary(wasmUrl?: string): Promise<{
            engineName: string;
            supportsInterrupts: boolean;
            supportsModuleLoader: boolean;
            workspaceRoot: string;
          }>;
        };
      }
    ).__runtimeHostSmoke;

    if (!runtimeHostSmoke) {
      throw new Error("runtime host smoke helper is unavailable");
    }

    return await runtimeHostSmoke.bootSummary("/runtime-host-qjs.wasm");
  });

  expect(summary).toEqual({
    engineName: "quickjs-ng-browser-c-vm",
    supportsInterrupts: true,
    supportsModuleLoader: true,
    workspaceRoot: "/workspace",
  });
});

test("launches the feature-enabled browser C VM wasm through launchRuntime smoke", async ({
  page,
}) => {
  await page.goto("/");

  const report = await page.evaluate(async () => {
    const runtimeHostSmoke = (
      window as Window & {
        __runtimeHostSmoke?: {
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
      }
    ).__runtimeHostSmoke;

    if (!runtimeHostSmoke) {
      throw new Error("runtime host smoke helper is unavailable");
    }

    return await runtimeHostSmoke.launchRuntime("/runtime-host-qjs.wasm");
  });

  expect(report.bootSummary.engineName).toBe("quickjs-ng-browser-c-vm");
  expect(report.engineContext.state).toBe("ready");
  expect(report.engineContext.bridgeReady).toBe(true);
  expect(report.engineContext.bootstrapSpecifier).toBe("runtime:bootstrap");
  expect(report.engineContext.registeredModules).toBeGreaterThan(0);
  expect(report.startupStdout).toContainEqual(
    expect.stringContaining("[engine-context] state=ready pending-jobs=0 bridge-ready=true"),
  );
  expect(report.previewReadyUrl).toContain("/preview/browser-smoke-session/");
});

test("resolves browser app createSession and mountSession through the worker control-plane", async ({
  page,
}) => {
  await page.goto("/");

  await page.evaluate(() => {
    const runtimeHostSmoke = (
      window as Window & {
        __runtimeHostSmoke?: {
          createAppSession?: () => Promise<unknown>;
        };
      }
    ).__runtimeHostSmoke;

    if (!runtimeHostSmoke?.createAppSession) {
      throw new Error("runtime app session smoke helpers are unavailable");
    }

    void runtimeHostSmoke.createAppSession().catch(() => undefined);
  });

  await expect
    .poll(
      async () =>
        await page.evaluate(() => {
          const runtimeHostSmoke = (
            window as Window & {
              __runtimeHostSmoke?: {
                appSessionState?: {
                  status: string;
                  phase?: string;
                  error?: string;
                };
              };
            }
          ).__runtimeHostSmoke;

          const state = runtimeHostSmoke?.appSessionState;
          if (state?.status === "rejected") {
            throw new Error(state.error ?? "createAppSession rejected");
          }

          return `${state?.status ?? "idle"}:${state?.phase ?? "none"}`;
        }),
      { timeout: 30000 },
    )
    .toBe("resolved:created");
});
