import { type ReactNode, useEffect, useEffectEvent, useRef, useState } from "react";

import { PreviewFrame } from "./components/PreviewFrame";
import { RuntimeController } from "./runtime/controller";
import type {
  PreviewDiagnostics,
  PreviewReadyEvent,
  PreviewWorkspaceFile,
  RunRequest,
  RuntimeEvent,
  SessionCreatedEvent,
  SessionSnapshot,
  SessionState,
} from "./runtime/protocol";
import {
  ensurePreviewServiceWorker,
  type PreviewBridgeRequestMessage,
  type PreviewBridgeResponseMessage,
  registerPreview,
  subscribePreviewWorkerState,
  unregisterPreview,
  unregisterSessionPreviews,
} from "./runtime/preview-service-worker";

type TerminalLine = {
  kind: "stdout" | "stderr" | "system";
  text: string;
};

type HostPreviewFileSelection = PreviewWorkspaceFile & {
  content: string;
};

const DEFAULT_REQUEST: RunRequest = {
  cwd: "/workspace",
  command: "npm",
  args: ["run", "dev"],
};

export function App() {
  const controllerRef = useRef<RuntimeController | null>(null);
  const [session, setSession] = useState<SessionSnapshot | null>(null);
  const [sessionState, setSessionState] = useState<SessionState>("booting");
  const [request, setRequest] = useState<RunRequest>(DEFAULT_REQUEST);
  const [terminal, setTerminal] = useState<TerminalLine[]>([
    {
      kind: "system",
      text: "Upload a project ZIP to mount /workspace and simulate a browser-side runtime session.",
    },
  ]);
  const [preview, setPreview] = useState<PreviewReadyEvent | null>(null);
  const [previewDiagnostics, setPreviewDiagnostics] = useState<PreviewDiagnostics | null>(null);
  const [previewDiagnosticsError, setPreviewDiagnosticsError] = useState<string | null>(null);
  const [previewFiles, setPreviewFiles] = useState<PreviewWorkspaceFile[] | null>(null);
  const [previewFileSelection, setPreviewFileSelection] = useState<HostPreviewFileSelection | null>(
    null,
  );
  const [previewFilesError, setPreviewFilesError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [previewRouter, setPreviewRouter] = useState("registering");
  const [previewRouterDetail, setPreviewRouterDetail] = useState<string | null>(null);

  if (!controllerRef.current) {
    controllerRef.current = new RuntimeController();
  }

  const controller = controllerRef.current;

  const appendTerminal = useEffectEvent((line: TerminalLine) => {
    setTerminal((current) => [...current, line]);
  });

  const applySessionCreated = useEffectEvent((event: SessionCreatedEvent) => {
    setSession(event.session);
    setSessionState(event.session.state);
    setPreview(null);
    setPreviewDiagnostics(null);
    setPreviewDiagnosticsError(null);
    setPreviewFiles(null);
    setPreviewFileSelection(null);
    setPreviewFilesError(null);
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

    if (!session || event.sessionId !== session.sessionId) {
      return;
    }

    switch (event.type) {
      case "session.state":
        setSessionState(event.state);
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
        setPreviewDiagnostics(null);
        setPreviewFiles(null);
        setPreviewFileSelection(null);
        setPreviewFilesError(null);
        setSessionState("stopped");
        break;
      case "preview.ready":
        setPreview(event);
        setPreviewDiagnostics(null);
        setPreviewDiagnosticsError(null);
        setPreviewFiles(null);
        setPreviewFileSelection(null);
        setPreviewFilesError(null);
        void registerPreview(event);
        appendTerminal({
          kind: "system",
          text: `Preview mapped to ${event.url}`,
        });
        break;
      case "runtime.error":
        appendTerminal({
          kind: "stderr",
          text: `${event.error.code}: ${event.error.message}`,
        });
        setPreviewDiagnostics(null);
        setPreviewDiagnosticsError(null);
        setPreviewFiles(null);
        setPreviewFileSelection(null);
        setPreviewFilesError(null);
        setSessionState("errored");
        break;
    }
  });

  useEffect(() => {
    const unsubscribe = controller.subscribe(handleRuntimeEvent);
    return () => {
      unsubscribe();
      controller.dispose();
    };
  }, [controller, handleRuntimeEvent]);

  useEffect(() => {
    void ensurePreviewServiceWorker();

    return subscribePreviewWorkerState((state) => {
      setPreviewRouter(state.status);
      setPreviewRouterDetail(state.detail ?? null);
    });
  }, []);

  useEffect(() => {
    if (!preview) {
      return;
    }

    let cancelled = false;

    void controller
      .requestPreviewResponse({
        sessionId: preview.sessionId,
        port: preview.port,
        method: "GET",
        pathname: `${preview.url}__diagnostics.json`,
        search: "",
        headers: {},
      })
      .then((response) => {
        if (cancelled) {
          return;
        }

        if (response.status >= 400) {
          setPreviewDiagnostics(null);
          setPreviewDiagnosticsError(`Failed to load diagnostics: ${response.status}`);
          return;
        }

        const payload = decodeVirtualHttpBody(response.body);
        setPreviewDiagnostics(JSON.parse(payload) as PreviewDiagnostics);
        setPreviewDiagnosticsError(null);
      })
      .catch((error: unknown) => {
        if (cancelled) {
          return;
        }

        setPreviewDiagnostics(null);
        setPreviewDiagnosticsError(
          error instanceof Error ? error.message : "Failed to load preview diagnostics.",
        );
      });

    return () => {
      cancelled = true;
    };
  }, [controller, preview]);

  useEffect(() => {
    if (!preview) {
      return;
    }

    let cancelled = false;

    void controller
      .requestPreviewJson<PreviewWorkspaceFile[]>(
        createPreviewRouteRequest(preview, `${preview.url}__files.json`),
      )
      .then(async (files) => {
        if (cancelled) {
          return;
        }

        setPreviewFiles(files);
        setPreviewFilesError(null);

        const preferredFile =
          files.find((file) => file.path.endsWith("/package.json")) ??
          files.find((file) => file.path.includes("/src/")) ??
          files[0];

        if (!preferredFile) {
          setPreviewFileSelection(null);
          return;
        }

        const content = await controller.requestPreviewText(
          createPreviewRouteRequest(preview, preferredFile.url),
        );

        if (cancelled) {
          return;
        }

        setPreviewFileSelection({
          ...preferredFile,
          content,
        });
      })
      .catch((error: unknown) => {
        if (cancelled) {
          return;
        }

        setPreviewFiles(null);
        setPreviewFileSelection(null);
        setPreviewFilesError(
          error instanceof Error ? error.message : "Failed to load preview file index.",
        );
      });

    return () => {
      cancelled = true;
    };
  }, [controller, preview]);

  useEffect(() => {
    if (!("serviceWorker" in navigator)) {
      return;
    }

    const onMessage = (event: MessageEvent<PreviewBridgeRequestMessage>) => {
      const message = event.data;
      const responsePort = event.ports[0];

      if (!responsePort || message?.type !== "preview.http.request") {
        return;
      }

      void controller
        .requestPreviewResponse(message.request)
        .then((response) => {
          responsePort.postMessage({
            type: "preview.http.response",
            requestId: message.requestId,
            response,
          } satisfies PreviewBridgeResponseMessage);
        })
        .catch((error: unknown) => {
          responsePort.postMessage({
            type: "preview.http.response",
            requestId: message.requestId,
            error: error instanceof Error ? error.message : "Unknown preview bridge error",
          } satisfies PreviewBridgeResponseMessage);
        });
    };

    navigator.serviceWorker.addEventListener("message", onMessage);

    return () => {
      navigator.serviceWorker.removeEventListener("message", onMessage);
    };
  }, [controller]);

  async function handleArchiveSelected(nextFile: File | undefined): Promise<void> {
    if (!nextFile) {
      return;
    }

    setIsBusy(true);

    try {
      if (session) {
        await unregisterSessionPreviews(session.sessionId);
      }

      const nextSession = await controller.createSession(nextFile);
      setSession(nextSession);
      setSessionState(nextSession.state);

      if (nextSession.packageJson?.scripts.dev) {
        setRequest({
          cwd: "/workspace",
          command: "npm",
          args: ["run", "dev"],
        });
      }
    } finally {
      setIsBusy(false);
    }
  }

  async function runSession(): Promise<void> {
    if (!session) {
      return;
    }

    setIsBusy(true);

    try {
      if (preview) {
        await unregisterPreview(preview.sessionId, preview.port);
      }

      setPreview(null);
      setPreviewDiagnostics(null);
      setPreviewDiagnosticsError(null);
      setPreviewFiles(null);
      setPreviewFileSelection(null);
      setPreviewFilesError(null);
      await controller.run(session.sessionId, request);
    } finally {
      setIsBusy(false);
    }
  }

  async function stopSession(): Promise<void> {
    if (!session) {
      return;
    }

    setIsBusy(true);

    try {
      if (preview) {
        await unregisterPreview(preview.sessionId, preview.port);
      }

      await controller.stop(session.sessionId);
      setPreview(null);
      setPreviewDiagnostics(null);
      setPreviewDiagnosticsError(null);
      setPreviewFiles(null);
      setPreviewFileSelection(null);
      setPreviewFilesError(null);
    } finally {
      setIsBusy(false);
    }
  }

  return (
    <main className="shell">
      <section className="hero-panel">
        <div>
          <p className="eyebrow">Browser-side node runner</p>
          <h1>React の中で別 React を起動するための最小実験場</h1>
          <p className="lede">
            AGENTS.md の責務分離に沿って、Main Thread、Runtime Worker、 仮想ワークスペース、preview
            を先に切り出したプロトタイプです。 いまは TypeScript
            実装のモックランタイムですが、Worker の内側をそのまま Rust/WASM + QuickJS
            に差し替えられる形にしています。
          </p>
        </div>
        <div className="status-cluster">
          <StatusPill label="State" value={sessionState} />
          <StatusPill label="Session" value={session?.sessionId.slice(0, 8) ?? "none"} />
          <StatusPill label="Files" value={String(session?.archive.fileCount ?? 0)} />
          <StatusPill label="Engine" value={preview?.host.engineName ?? "pending"} />
          <StatusPill label="Preview router" value={previewRouter} />
        </div>
      </section>

      <section className="workspace-grid">
        <div className="panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">1. Session</p>
              <h2>ZIP を mount する</h2>
            </div>
          </div>

          <label className="file-drop">
            <span>project.zip を選択</span>
            <input
              accept=".zip,application/zip"
              type="file"
              onChange={(event) => handleArchiveSelected(event.currentTarget.files?.[0])}
            />
          </label>

          {session ? (
            <div className="meta-grid">
              <MetaRow label="Archive">{session.archive.fileName}</MetaRow>
              <MetaRow label="Workspace root">{session.workspaceRoot}</MetaRow>
              <MetaRow label="Entries">
                {session.archive.fileCount} files / {session.archive.directoryCount} directories
              </MetaRow>
              <MetaRow label="React detected">
                {session.capabilities.detectedReact ? "yes" : "no"}
              </MetaRow>
              <MetaRow label="Scripts">
                {Object.keys(session.packageJson?.scripts ?? {}).join(", ") || "none"}
              </MetaRow>
            </div>
          ) : (
            <p className="empty-copy">
              node_modules 同梱 ZIP を読み込むと、Worker 側で package.json
              とエントリ一覧を解析します。
            </p>
          )}
        </div>

        <div className="panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">2. Runtime Worker</p>
              <h2>起動コマンド</h2>
            </div>
          </div>

          <div className="form-grid">
            <label>
              <span>CWD</span>
              <input
                value={request.cwd}
                onChange={(event) =>
                  setRequest((current) => ({
                    ...current,
                    cwd: event.currentTarget.value,
                  }))
                }
              />
            </label>
            <label>
              <span>Command</span>
              <input
                value={request.command}
                onChange={(event) =>
                  setRequest((current) => ({
                    ...current,
                    command: event.currentTarget.value,
                  }))
                }
              />
            </label>
            <label className="full">
              <span>Args</span>
              <input
                value={request.args.join(" ")}
                onChange={(event) =>
                  setRequest((current) => ({
                    ...current,
                    args: splitArgs(event.currentTarget.value),
                  }))
                }
              />
            </label>
          </div>

          <div className="action-row">
            <button
              className="primary"
              disabled={!session || isBusy}
              onClick={() => void runSession()}
            >
              Run session
            </button>
            <button
              className="secondary"
              disabled={!session || isBusy || sessionState !== "running"}
              onClick={() => void stopSession()}
            >
              Stop session
            </button>
            {session?.packageJson?.scripts.dev ? (
              <button
                className="ghost"
                disabled={isBusy}
                onClick={() =>
                  setRequest({
                    cwd: "/workspace",
                    command: "npm",
                    args: ["run", "dev"],
                  })
                }
              >
                Use npm run dev
              </button>
            ) : null}
          </div>

          <p className="help-copy">
            いまは `npm run &lt;script&gt;` と `node &lt;entry&gt;` を模擬解釈します。 ここが将来の
            QuickJS + Node-like host API の入口です。
          </p>
        </div>

        <div className="panel terminal-panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">3. Stdout / stderr</p>
              <h2>ターミナル</h2>
            </div>
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
        </div>

        <div className="panel preview-panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">4. Preview</p>
              <h2>iframe 内で guest React を起動</h2>
            </div>
            <code>{preview?.url ?? "/preview/<session>/<port>/"}</code>
          </div>

          {previewRouterDetail ? <p className="router-note">{previewRouterDetail}</p> : null}
          <PreviewFrame preview={preview} serviceWorkerReady={previewRouter === "ready"} />
        </div>

        <div className="panel runtime-panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">5. Run plan</p>
              <h2>Rust host が返した実行計画</h2>
            </div>
          </div>

          {preview ? (
            <div className="meta-grid">
              <MetaRow label="Command kind">{preview.run.commandKind}</MetaRow>
              <MetaRow label="Command line">{preview.run.commandLine}</MetaRow>
              <MetaRow label="Resolved cwd">{preview.run.cwd}</MetaRow>
              <MetaRow label="Entrypoint">{preview.run.entrypoint}</MetaRow>
              <MetaRow label="Resolved script">
                {preview.run.resolvedScript ?? "<direct-entry>"}
              </MetaRow>
              <MetaRow label="Host engine">{preview.host.engineName}</MetaRow>
              <MetaRow label="Host VFS">
                {preview.hostFiles.count} files / {preview.hostFiles.samplePath ?? "none"}
              </MetaRow>
            </div>
          ) : (
            <p className="empty-copy">
              preview.ready が返ると、ここに Rust/WASM host 側で正規化された run plan と host
              概要を表示します。
            </p>
          )}
        </div>

        <div className="panel diagnostics-panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">6. Diagnostics</p>
              <h2>preview 内部状態をホスト側で観測</h2>
            </div>
          </div>

          {previewDiagnostics ? (
            <div className="meta-grid">
              <MetaRow label="Root hint">
                {previewDiagnostics.rootRequestHint?.kind ?? "none"}
              </MetaRow>
              <MetaRow label="Request hint">
                {previewDiagnostics.requestHint?.kind ?? "none"}
              </MetaRow>
              <MetaRow label="Hydrated files">
                {previewDiagnostics.hydratedFileCount} / {previewDiagnostics.fileCount}
              </MetaRow>
              <MetaRow label="Hydrated paths">
                {previewDiagnostics.hydratedPaths.slice(0, 3).join(", ") || "none"}
              </MetaRow>
              <MetaRow label="Diagnostics URL">
                {`${previewDiagnostics.url}__diagnostics.json`}
              </MetaRow>
            </div>
          ) : (
            <p className="empty-copy">
              {previewDiagnosticsError ??
                "preview 実行後に __diagnostics.json を読み、request hint と hydration 状態をここへ表示します。"}
            </p>
          )}
        </div>

        <div className="panel inspector-panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">7. Preview inspector</p>
              <h2>ホスト側から preview 配信を検査</h2>
            </div>
          </div>

          {previewFiles && previewFileSelection ? (
            <div className="meta-grid">
              <MetaRow label="Indexed files">{String(previewFiles.length)}</MetaRow>
              <MetaRow label="Selected file">{previewFileSelection.path}</MetaRow>
              <MetaRow label="Content type">{previewFileSelection.contentType}</MetaRow>
              <MetaRow label="Preview route">{previewFileSelection.previewUrl}</MetaRow>
              <MetaRow label="Raw route">{previewFileSelection.url}</MetaRow>
              <MetaRow label="Body preview">
                <code>{truncateInspectorSource(previewFileSelection.content)}</code>
              </MetaRow>
            </div>
          ) : (
            <p className="empty-copy">
              {previewFilesError ??
                "preview 実行後に __files.json と実ファイル本文を読み、ホスト側からも preview ルートを検査します。"}
            </p>
          )}
        </div>

        <div className="panel architecture-panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">WASM seam</p>
              <h2>次に差し替える場所</h2>
            </div>
          </div>

          <ol className="checkpoint-list">
            <li>ZIP 解析を Rust/WASM 側 VFS mount に置き換える</li>
            <li>Worker の run 解釈部を QuickJS プロセス起動へ置き換える</li>
            <li>preview.ready を Service Worker 経由の本物の URL 配信に置き換える</li>
            <li>fs / path / process を Node-like host API として段階実装する</li>
          </ol>
        </div>

        <div className="panel file-panel">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">Archive map</p>
              <h2>仮想ワークスペースの入口</h2>
            </div>
          </div>

          {session ? (
            <ul className="entry-list">
              {session.archive.entries.slice(0, 18).map((entry) => (
                <li key={entry.path}>
                  <code>{entry.path}</code>
                  <span>{entry.kind}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="empty-copy">ここに ZIP 内のエントリ先頭が表示されます。</p>
          )}
        </div>
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

function decodeVirtualHttpBody(body: string | Uint8Array): string {
  if (typeof body === "string") {
    return body;
  }

  return new TextDecoder().decode(body);
}

function createPreviewRouteRequest(
  preview: PreviewReadyEvent,
  pathname: string,
): {
  sessionId: string;
  port: number;
  method: string;
  pathname: string;
  search: string;
  headers: Record<string, string>;
} {
  return {
    sessionId: preview.sessionId,
    port: preview.port,
    method: "GET",
    pathname,
    search: "",
    headers: {},
  };
}

function truncateInspectorSource(source: string): string {
  return source.split("\n").slice(0, 4).join(" ").slice(0, 140);
}

function StatusPill(props: { label: string; value: string }) {
  return (
    <div className="status-pill">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  );
}

function MetaRow(props: { label: string; children: ReactNode }) {
  return (
    <div className="meta-row">
      <span>{props.label}</span>
      <strong>{props.children}</strong>
    </div>
  );
}
