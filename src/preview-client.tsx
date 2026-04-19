import ReactDOM from "react-dom/client";

import { PreviewApp } from "./preview/PreviewApp";
import type { PreviewReadyEvent, PreviewWorkspaceFile, SessionSnapshot } from "./runtime/protocol";
import "./style.css";

type PreviewBootstrap = {
  sessionId: string;
  port: number;
  stateUrl: string;
  workspaceUrl: string;
  filesUrl: string;
};

declare global {
  interface Window {
    __NODE_IN_NODE_PREVIEW__?: PreviewBootstrap;
  }
}

async function main(): Promise<void> {
  const rootElement = document.querySelector<HTMLDivElement>("#guest-root");

  if (!rootElement) {
    throw new Error("Missing #guest-root");
  }

  const bootstrap = window.__NODE_IN_NODE_PREVIEW__;

  if (!bootstrap) {
    throw new Error("Missing preview bootstrap payload");
  }

  const [previewResponse, workspaceResponse, filesResponse] = await Promise.all([
    fetch(bootstrap.stateUrl, { cache: "no-store" }),
    fetch(bootstrap.workspaceUrl, { cache: "no-store" }),
    fetch(bootstrap.filesUrl, { cache: "no-store" }),
  ]);

  if (!previewResponse.ok) {
    throw new Error(`Failed to load preview data: ${previewResponse.status}`);
  }

  if (!workspaceResponse.ok) {
    throw new Error(`Failed to load workspace data: ${workspaceResponse.status}`);
  }

  if (!filesResponse.ok) {
    throw new Error(`Failed to load file index: ${filesResponse.status}`);
  }

  const [preview, workspace, files] = (await Promise.all([
    previewResponse.json(),
    workspaceResponse.json(),
    filesResponse.json(),
  ])) as [PreviewReadyEvent, SessionSnapshot, PreviewWorkspaceFile[]];

  const preferredFile =
    files.find((file) => file.path.endsWith("/package.json")) ??
    files.find((file) => file.path.includes("/src/")) ??
    files[0];

  const selectedFile = preferredFile
    ? await fetch(preferredFile.url, { cache: "no-store" }).then(async (response) => {
        if (!response.ok) {
          throw new Error(`Failed to load file ${preferredFile.path}: ${response.status}`);
        }

        return {
          ...preferredFile,
          content: await response.text(),
        };
      })
    : null;

  ReactDOM.createRoot(rootElement).render(
    <PreviewApp
      files={files}
      preview={preview}
      selectedFile={selectedFile}
      workspace={workspace}
    />,
  );
}

void main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : "Unknown preview bootstrap error";
  document.body.innerHTML = `<main style="display:grid;place-items:center;min-height:100vh;margin:0;background:#0f172a;color:#e2e8f0;font-family:system-ui,sans-serif;">
    <article style="width:min(560px,calc(100% - 32px));padding:24px;border-radius:24px;background:rgba(15,23,42,0.78);border:1px solid rgba(148,163,184,0.2);">
      <h1 style="margin:0 0 12px;">Preview bootstrap failed</h1>
      <p style="margin:0;">${escapeHtml(message)}</p>
    </article>
  </main>`;
});

function escapeHtml(input: string): string {
  return input
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
