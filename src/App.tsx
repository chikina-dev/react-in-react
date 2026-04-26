import { useEffect, useEffectEvent, useRef, useState } from "react";

import { PreviewFrame } from "./components/PreviewFrame";
import { installRuntimeAppSmoke } from "./runtime/app-smoke";
import { RuntimeController } from "./runtime/controller";
import { withAppBasePath } from "./runtime/app-base";
import type {
  PreviewReadyEvent,
  RunRequest,
  RuntimeProgressEvent,
  RuntimeEvent,
  SessionCreatedEvent,
  SessionSnapshot,
  SessionState,
} from "./runtime/protocol";

type TerminalLine = {
  kind: "stdout" | "stderr" | "system";
  text: string;
};

const DEFAULT_REQUEST: RunRequest = {
  cwd: "/workspace",
  command: "npm",
  args: ["run", "dev"],
};
const DEFAULT_REQUEST_ARGS_TEXT = DEFAULT_REQUEST.args.join(" ");

export function App() {
  const [controller] = useState(() => new RuntimeController());
  const [session, setSession] = useState<SessionSnapshot | null>(null);
  const currentSessionIdRef = useRef<string | null>(null);
  const [sessionState, setSessionState] = useState<SessionState>("booting");
  const [request, setRequest] = useState<RunRequest>(DEFAULT_REQUEST);
  const [requestArgsText, setRequestArgsText] = useState(DEFAULT_REQUEST_ARGS_TEXT);
  const [terminal, setTerminal] = useState<TerminalLine[]>([
    {
      kind: "system",
      text: "Upload a project ZIP to mount /workspace and simulate a browser-side runtime session.",
    },
  ]);
  const [preview, setPreview] = useState<PreviewReadyEvent | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [progress, setProgress] = useState<RuntimeProgressEvent | null>(null);
  const [previewRouter, setPreviewRouter] = useState("registering");
  const [previewRouterDetail, setPreviewRouterDetail] = useState<string | null>(null);

  const appendTerminal = useEffectEvent((line: TerminalLine) => {
    setTerminal((current) => [...current, line]);
  });

  const applySuggestedRunRequest = useEffectEvent((nextRequest: RunRequest | null | undefined) => {
    if (!nextRequest) {
      return;
    }

    setRequest({
      cwd: nextRequest.cwd,
      command: nextRequest.command,
      args: [...nextRequest.args],
      env: nextRequest.env ? { ...nextRequest.env } : undefined,
    });
    setRequestArgsText(nextRequest.args.join(" "));
  });

  const applyPreviewReady = useEffectEvent((event: PreviewReadyEvent) => {
    setPreview(event);
    setProgress(null);
    appendTerminal({
      kind: "system",
      text: `Preview mapped to ${event.url}`,
    });
  });

  const applySessionCreated = useEffectEvent((event: SessionCreatedEvent) => {
    currentSessionIdRef.current = event.session.sessionId;
    setSession(event.session);
    setSessionState(event.session.state);
    setPreview(null);
    setProgress(null);
    applySuggestedRunRequest(event.session.suggestedRunRequest);
    setTerminal([
      {
        kind: "system",
        text: `Mounted ${event.session.archive.fileCount} files from ${event.session.archive.fileName} into ${event.session.workspaceRoot}.`,
      },
    ]);
  });

  const handleRuntimeEvent = useEffectEvent((event: RuntimeEvent) => {
    if (event.type === "session.created") {
      applySessionCreated(event);
      return;
    }

    const activeSessionId = currentSessionIdRef.current;

    if (!activeSessionId || event.sessionId !== activeSessionId) {
      return;
    }

    switch (event.type) {
      case "session.state":
        setSessionState(event.state);
        if (event.state === "stopped") {
          setProgress(null);
        }
        break;
      case "runtime.progress":
        setProgress(event);
        appendTerminal({
          kind: "system",
          text: formatProgressText(event),
        });
        break;
      case "process.stdout":
        appendTerminal({ kind: "stdout", text: event.chunk });
        break;
      case "process.stderr":
        appendTerminal({ kind: "stderr", text: event.chunk });
        break;
      case "process.exit":
        appendTerminal({
          kind: "system",
          text: `Process ${event.pid} exited with code ${event.code}.`,
        });
        setSessionState("stopped");
        setProgress(null);
        break;
      case "preview.ready":
        applyPreviewReady(event);
        break;
      case "runtime.error":
        setProgress(null);
        appendTerminal({
          kind: "stderr",
          text: `${event.error.code}: ${event.error.message}`,
        });
        setSessionState("errored");
        break;
    }
  });

  useEffect(() => {
    const unsubscribe = controller.subscribe((event) => {
      handleRuntimeEvent(event);
    });
    return unsubscribe;
  }, [controller, handleRuntimeEvent]);

  useEffect(() => {
    return () => {
      controller.dispose();
    };
  }, [controller]);

  useEffect(() => {
    void controller.ensurePreviewWorker();

    return controller.subscribePreviewWorkerState((state) => {
      setPreviewRouter(state.status);
      setPreviewRouterDetail(state.detail ?? null);
    });
  }, [controller]);

  useEffect(() => {
    return installRuntimeAppSmoke(controller);
  }, [controller]);

  async function handleArchiveSelected(nextFile: File | undefined): Promise<void> {
    if (!nextFile) {
      return;
    }

    setIsBusy(true);
    setProgress({
      type: "runtime.progress",
      sessionId: currentSessionIdRef.current ?? "pending-session",
      stage: "session-create",
      message: `Preparing ${nextFile.name}...`,
      values: {
        fileName: nextFile.name,
      },
    });

    try {
      currentSessionIdRef.current = null;
      setPreview(null);
      const nextSession = await controller.replaceSession(session?.sessionId ?? null, nextFile);
      currentSessionIdRef.current = nextSession.sessionId;
      setSession(nextSession);
      setSessionState(nextSession.state);
    } finally {
      setIsBusy(false);
    }
  }

  async function runSession(): Promise<void> {
    if (!session) {
      return;
    }

    setIsBusy(true);
    setProgress({
      type: "runtime.progress",
      sessionId: session.sessionId,
      stage: "runtime-launch",
      message: "Submitting run request...",
      values: {
        cwd: request.cwd,
        command: request.command,
        args: requestArgsText,
      },
    });

    try {
      setPreview(null);
      await controller.run(session.sessionId, {
        ...request,
        args: splitArgs(requestArgsText),
      });
    } finally {
      setIsBusy(false);
    }
  }

  async function stopSession(): Promise<void> {
    if (!session) {
      return;
    }

    setIsBusy(true);
    setProgress({
      type: "runtime.progress",
      sessionId: session.sessionId,
      stage: "session-stop",
      message: "Stopping session...",
    });

    try {
      await controller.stop(session.sessionId);
      currentSessionIdRef.current = null;
      setPreview(null);
    } finally {
      setIsBusy(false);
    }
  }

  return (
    <main className="app-shell">
      <h1 className="app-title">node-in-node</h1>

      <section className="panel">
        <label className="section-label">
          <span>ZIP</span>
          <div className="file-drop">
            <span>{session?.archive.fileName ?? "project.zip を選択"}</span>
            <input
              accept=".zip,application/zip"
              type="file"
              onChange={(event) => handleArchiveSelected(event.currentTarget.files?.[0])}
            />
          </div>
        </label>
      </section>

      <section className="panel">
        <div className="command-grid">
          <label className="section-label">
            <span>CWD</span>
            <input
              value={request.cwd}
              onChange={(event) => {
                const nextCwd = event.currentTarget.value;
                setRequest((current) => ({
                  ...current,
                  cwd: nextCwd,
                }));
              }}
            />
          </label>
          <label className="section-label">
            <span>Command</span>
            <input
              value={request.command}
              onChange={(event) => {
                const nextCommand = event.currentTarget.value;
                setRequest((current) => ({
                  ...current,
                  command: nextCommand,
                }));
              }}
            />
          </label>
          <label className="section-label">
            <span>Args</span>
            <input
              value={requestArgsText}
              onChange={(event) => {
                const nextArgsText = event.currentTarget.value;
                setRequest((current) => ({
                  ...current,
                  args: splitArgs(nextArgsText),
                }));
                setRequestArgsText(nextArgsText);
              }}
            />
          </label>
        </div>

        <div className="action-row">
          <button disabled={!session || isBusy} onClick={() => void runSession()}>
            Run session
          </button>
          <button
            disabled={!session || isBusy || sessionState !== "running"}
            onClick={() => void stopSession()}
          >
            Stop session
          </button>
          {session?.suggestedRunRequest ? (
            <button
              disabled={isBusy}
              onClick={() => {
                applySuggestedRunRequest(session.suggestedRunRequest);
              }}
            >
              Use {formatRunRequest(session.suggestedRunRequest)}
            </button>
          ) : null}
        </div>
        {progress ? (
          <div className="muted">
            <p>Loading: {formatProgressText(progress)}</p>
            {progress.values ? <code>{formatProgressValues(progress.values)}</code> : null}
          </div>
        ) : null}
      </section>

      <section className="panel preview-panel">
        <div className="section-heading">
          <span>Preview</span>
          <code>{preview?.url ?? withAppBasePath("/preview/<session>/<port>/")}</code>
        </div>
        {previewRouterDetail ? <p className="muted">{previewRouterDetail}</p> : null}
        <PreviewFrame preview={preview} serviceWorkerReady={previewRouter === "ready"} />
      </section>

      <section className="panel">
        <div className="section-heading">
          <span>Terminal</span>
        </div>
        <pre className="terminal" aria-live="polite">
          {terminal.map((line, index) => (
            <div key={`${line.kind}-${index}`} className={`line ${line.kind}`}>
              <span className="prompt">
                {line.kind === "stderr" ? "!" : line.kind === "system" ? "*" : ">"}
              </span>
              <span>{line.text}</span>
            </div>
          ))}
        </pre>
      </section>
    </main>
  );
}

function splitArgs(raw: string): string[] {
  return raw
    .split(" ")
    .map((part) => part.trim())
    .filter(Boolean);
}

function formatRunRequest(request: RunRequest): string {
  return [request.command, ...request.args].join(" ");
}

function formatProgressText(progress: RuntimeProgressEvent): string {
  const values = progress.values ? formatProgressValues(progress.values) : null;
  return values ? `${progress.message} (${values})` : progress.message;
}

function formatProgressValues(values: Record<string, string | number | boolean | null>): string {
  return Object.entries(values)
    .filter(([, value]) => value != null && value !== "")
    .map(([key, value]) => `${key}=${String(value)}`)
    .join(", ");
}
