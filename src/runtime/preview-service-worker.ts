import previewClientUrl from "../preview-client.tsx?url";
import type { PreviewReadyEvent, VirtualHttpRequest, VirtualHttpResponse } from "./protocol";
import { PREVIEW_CLIENT_HEADER } from "./preview-constants";

type PreviewWorkerStatus = "unsupported" | "registering" | "ready" | "error";

type PreviewWorkerState = {
  status: PreviewWorkerStatus;
  detail?: string;
};

type PreviewWorkerMessage =
  | {
      type: "preview.configure";
      clientScriptUrl: string;
    }
  | {
      type: "preview.register";
      preview: PreviewReadyEvent;
    }
  | {
      type: "preview.unregister";
      sessionId: string;
      port: number;
    }
  | {
      type: "preview.unregister-all";
      sessionId: string;
    };

export type PreviewBridgeRequestMessage = {
  type: "preview.http.request";
  requestId: string;
  request: VirtualHttpRequest;
};

export type PreviewBridgeResponseMessage = {
  type: "preview.http.response";
  requestId: string;
  response?: VirtualHttpResponse;
  error?: string;
};

let registrationPromise: Promise<ServiceWorkerRegistration | null> | null = null;
let currentState: PreviewWorkerState = { status: "registering" };
const listeners = new Set<(state: PreviewWorkerState) => void>();
const queue: PreviewWorkerMessage[] = [];

export function subscribePreviewWorkerState(
  listener: (state: PreviewWorkerState) => void,
): () => void {
  listeners.add(listener);
  listener(currentState);

  return () => {
    listeners.delete(listener);
  };
}

export async function ensurePreviewServiceWorker(): Promise<ServiceWorkerRegistration | null> {
  if (typeof window === "undefined" || !("serviceWorker" in navigator)) {
    updateState({
      status: "unsupported",
      detail: "Service Worker is unavailable in this browser.",
    });
    return null;
  }

  if (!registrationPromise) {
    registrationPromise = registerPreviewServiceWorker();
  }

  return registrationPromise;
}

export async function registerPreview(preview: PreviewReadyEvent): Promise<void> {
  await ensurePreviewServiceWorker();
  await postToServiceWorker({
    type: "preview.register",
    preview,
  });
}

export async function unregisterPreview(sessionId: string, port: number): Promise<void> {
  await ensurePreviewServiceWorker();
  await postToServiceWorker({
    type: "preview.unregister",
    sessionId,
    port,
  });
}

export async function unregisterSessionPreviews(sessionId: string): Promise<void> {
  await ensurePreviewServiceWorker();
  await postToServiceWorker({
    type: "preview.unregister-all",
    sessionId,
  });
}

async function registerPreviewServiceWorker(): Promise<ServiceWorkerRegistration | null> {
  try {
    const registration = await navigator.serviceWorker.register("/preview-sw.js", {
      scope: "/",
    });

    await navigator.serviceWorker.ready;
    postMessageToTarget(
      registration.active ??
        registration.waiting ??
        registration.installing ??
        navigator.serviceWorker.controller,
      {
        type: "preview.configure",
        clientScriptUrl: previewClientUrl,
      },
    );

    updateState({ status: "ready" });
    flushQueue();

    return registration;
  } catch (error) {
    updateState({
      status: "error",
      detail: error instanceof Error ? error.message : "Service Worker registration failed.",
    });
    return null;
  }
}

async function postToServiceWorker(message: PreviewWorkerMessage): Promise<void> {
  const registration = await ensurePreviewServiceWorker();

  if (!registration) {
    return;
  }

  const target =
    registration.active ??
    registration.waiting ??
    registration.installing ??
    navigator.serviceWorker.controller;

  if (!target) {
    queue.push(message);
    return;
  }

  postMessageToTarget(target, message);
}

function flushQueue(): void {
  const pending = [...queue];
  queue.length = 0;

  for (const message of pending) {
    void postToServiceWorker(message);
  }
}

function updateState(nextState: PreviewWorkerState): void {
  currentState = nextState;

  for (const listener of listeners) {
    listener(nextState);
  }
}

function postMessageToTarget(
  target: ServiceWorker | null | undefined,
  message: PreviewWorkerMessage,
): void {
  if (!target) {
    queue.push(message);
    return;
  }

  target.postMessage(message);
}

export function withPreviewClientHeader(headers: Record<string, string>): Record<string, string> {
  return {
    ...headers,
    [PREVIEW_CLIENT_HEADER]: previewClientUrl,
  };
}
