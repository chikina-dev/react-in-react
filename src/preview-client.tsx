import ReactDOM from "react-dom/client";

import { PreviewApp } from "./preview/PreviewApp";
import type { PreviewBootstrapPayload } from "./runtime/protocol";
import "./style.css";

type PreviewBootstrap = {
  sessionId: string;
  port: number;
  bootstrapUrl: string;
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

  const bootstrapResponse = await fetch(bootstrap.bootstrapUrl, { cache: "no-store" });
  if (!bootstrapResponse.ok) {
    throw new Error(`Failed to load preview bootstrap: ${bootstrapResponse.status}`);
  }
  const payload = (await bootstrapResponse.json()) as PreviewBootstrapPayload;

  ReactDOM.createRoot(rootElement).render(
    <PreviewApp
      files={payload.files}
      preview={payload.preview}
      diagnostics={payload.diagnostics}
      selectedFile={payload.selectedFile}
      workspace={payload.workspace}
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
