import { strToU8, zipSync } from "fflate";

import { mountArchive } from "./analyze-archive";
import type { RuntimeController } from "./controller";
import type { PreviewReadyEvent, RunRequest, SessionSnapshot, SessionState } from "./protocol";
import { withPreviewClientHeader } from "./preview-service-worker";

type RuntimeAppSmokeWindow = Window & {
  __runtimeHostSmoke?: {
    launchAppPreviewState?: {
      status: "idle" | "running" | "resolved" | "rejected";
      phase?: string;
      result?: {
        sessionId: string;
        url: string;
        status: number;
        bodyPreview: string;
        stdout: string[];
      };
      error?: string;
    };
    appSessionState?: {
      status: "idle" | "running" | "resolved" | "rejected";
      phase?: string;
      result?: {
        created?: {
          sessionId: string;
          state: SessionState;
          fileCount: number;
          workspaceRoot: string;
        };
        mounted?: {
          sessionId: string;
          state: SessionState;
          fileCount: number;
          workspaceRoot: string;
        };
      };
      error?: string;
    };
    appPreviewTrace?: string[];
    launchAppPreview?: () => Promise<{
      sessionId: string;
      url: string;
      status: number;
      bodyPreview: string;
      stdout: string[];
    }>;
    launchAppPreviewTask?: Promise<{
      sessionId: string;
      url: string;
      status: number;
      bodyPreview: string;
      stdout: string[];
    }>;
    createAppSession?: () => Promise<{
      sessionId: string;
      state: SessionState;
      fileCount: number;
      workspaceRoot: string;
    }>;
    mountAppSession?: () => Promise<{
      sessionId: string;
      state: SessionState;
      fileCount: number;
      workspaceRoot: string;
    }>;
    runAppPreview?: () => void;
  };
};

type AppPreviewResult = {
  sessionId: string;
  url: string;
  status: number;
  bodyPreview: string;
  stdout: string[];
};

function createSmokeArchiveFile(kind: "session" | "preview" = "session"): File {
  const archive =
    kind === "preview"
      ? zipSync({
          "package.json": strToU8('{"name":"browser-preview-smoke"}'),
          "src/server.js": strToU8("console.log('browser');"),
        })
      : zipSync({
          "package.json": strToU8('{"name":"browser-session-smoke"}'),
          "src/index.js": strToU8("console.log('browser session smoke');"),
        });
  const bytes = new Uint8Array(archive);
  return new File([bytes], `browser-${kind}-smoke.zip`, {
    type: "application/zip",
  });
}

function toSessionResult(session: SessionSnapshot) {
  return {
    sessionId: session.sessionId,
    state: session.state,
    fileCount: session.archive.fileCount,
    workspaceRoot: session.workspaceRoot,
  };
}

export function installRuntimeAppSmoke(controller: RuntimeController): () => void {
  const smokeWindow = window as RuntimeAppSmokeWindow;
  const existing = smokeWindow.__runtimeHostSmoke ?? {};

  smokeWindow.__runtimeHostSmoke = {
    ...existing,
    launchAppPreviewState: existing.launchAppPreviewState ?? { status: "idle" },
    appPreviewTrace: existing.appPreviewTrace ?? [],
    createAppSession: async () => {
      try {
        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "running",
          phase: "creating-archive",
        };
        const file = createSmokeArchiveFile();
        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "running",
          phase: "awaiting-create-session",
        };
        const session = await controller.createSession(file);

        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "resolved",
          phase: "created",
          result: {
            created: toSessionResult(session),
          },
        };
        return toSessionResult(session);
      } catch (error) {
        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "rejected",
          phase: "create-failed",
          error: error instanceof Error ? error.message : "Unknown create session error",
        };
        throw error;
      }
    },
    mountAppSession: async () => {
      try {
        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "running",
          phase: "creating-archive",
        };
        const file = createSmokeArchiveFile();
        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "running",
          phase: "mounting-archive",
        };
        const mounted = mountArchive(file.name, await file.arrayBuffer(), crypto.randomUUID());
        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "running",
          phase: "awaiting-mount-session",
        };
        const session = await controller.mountSession(mounted);

        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "resolved",
          phase: "mounted",
          result: {
            mounted: toSessionResult(session),
          },
        };
        return toSessionResult(session);
      } catch (error) {
        smokeWindow.__runtimeHostSmoke!.appSessionState = {
          status: "rejected",
          phase: "mount-failed",
          error: error instanceof Error ? error.message : "Unknown mount session error",
        };
        throw error;
      }
    },
    launchAppPreview: async (): Promise<AppPreviewResult> => {
      smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push("launch:start");
      smokeWindow.__runtimeHostSmoke!.launchAppPreviewState = {
        status: "running",
        phase: "fetching-archive",
      };
      const file = createSmokeArchiveFile("preview");
      smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push(`archive:${file.name}:${file.size}`);
      const stdout: string[] = [];

      const previewReadyPromise = new Promise<PreviewReadyEvent>((resolve, reject) => {
        const unsubscribeWorkerError = controller.subscribeWorkerErrors((error) => {
          unsubscribe();
          unsubscribeWorkerError();
          smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push(`worker-error:${error.message}`);
          reject(error);
        });
        const unsubscribe = controller.subscribe((event) => {
          if (event.type === "process.stdout") {
            stdout.push(event.chunk);
            smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push(`stdout:${event.chunk}`);
          }
          if (event.type === "runtime.error") {
            unsubscribe();
            unsubscribeWorkerError();
            smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push(
              `runtime-error:${event.error.code}:${event.error.message}`,
            );
            reject(new Error(`${event.error.code}: ${event.error.message}`));
            return;
          }
          if (event.type === "preview.ready") {
            unsubscribe();
            unsubscribeWorkerError();
            smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push(`preview-ready:${event.url}`);
            resolve(event);
          }
        });
      });

      smokeWindow.__runtimeHostSmoke!.launchAppPreviewState = {
        status: "running",
        phase: "creating-session",
      };
      const created = await controller.createSession(file);
      smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push(`session-created:${created.sessionId}`);
      smokeWindow.__runtimeHostSmoke!.launchAppPreviewState = {
        status: "running",
        phase: "running-session",
      };
      smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push("run:dispatch");
      await controller.run(created.sessionId, {
        cwd: "/workspace",
        command: "node",
        args: ["/workspace/src/server.js"],
      } satisfies RunRequest);
      smokeWindow.__runtimeHostSmoke!.appPreviewTrace?.push("run:returned");
      smokeWindow.__runtimeHostSmoke!.launchAppPreviewState = {
        status: "running",
        phase: "waiting-preview-ready",
      };
      const ready = await previewReadyPromise;
      try {
        smokeWindow.__runtimeHostSmoke!.launchAppPreviewState = {
          status: "running",
          phase: "fetching-preview",
        };
        const response = await fetch(ready.url, {
          headers: withPreviewClientHeader({}),
        });
        const body = await response.text();
        return {
          sessionId: created.sessionId,
          url: ready.url,
          status: response.status,
          bodyPreview: body.slice(0, 2000),
          stdout,
        };
      } finally {
        await controller.stop(created.sessionId).catch(() => undefined);
      }
    },
    runAppPreview: () => {
      smokeWindow.__runtimeHostSmoke!.launchAppPreviewState = {
        status: "running",
        phase: "starting",
      };
      smokeWindow.__runtimeHostSmoke!.launchAppPreviewTask = smokeWindow
        .__runtimeHostSmoke!.launchAppPreview?.()
        .then((result) => {
          smokeWindow.__runtimeHostSmoke!.launchAppPreviewState = {
            status: "resolved",
            phase: "resolved",
            result,
          };
          return result;
        })
        .catch((error: unknown) => {
          smokeWindow.__runtimeHostSmoke!.launchAppPreviewState = {
            status: "rejected",
            phase: "rejected",
            error: error instanceof Error ? error.message : "Unknown app preview smoke error",
          };
          throw error;
        });
    },
  };

  return () => {
    const current = smokeWindow.__runtimeHostSmoke;
    if (current?.launchAppPreview) {
      delete current.launchAppPreview;
    }
    if (current?.createAppSession) {
      delete current.createAppSession;
    }
    if (current?.mountAppSession) {
      delete current.mountAppSession;
    }
    if (current?.runAppPreview) {
      delete current.runAppPreview;
    }
    if (current?.launchAppPreviewTask) {
      delete current.launchAppPreviewTask;
    }
    if (current?.launchAppPreviewState) {
      delete current.launchAppPreviewState;
    }
    if (current?.appSessionState) {
      delete current.appSessionState;
    }
    if (current?.appPreviewTrace) {
      delete current.appPreviewTrace;
    }
  };
}
