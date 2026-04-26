import previewClientUrl from "../preview-client.tsx?url";
import { APP_BASE_PATH, withAppBasePath } from "./app-base";
import type { PreviewReadyEvent, VirtualHttpRequest, VirtualHttpResponse } from "./protocol";
import { PREVIEW_CLIENT_HEADER } from "./preview-constants";

type PreviewWorkerStatus = "unsupported" | "registering" | "ready" | "error";

export type PreviewWorkerState = {
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

type PreviewBridgeConnectMessage = {
  type: "preview.bridge.connect";
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
const queue: Array<PreviewWorkerMessage | PreviewBridgeConnectMessage> = [];

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

export async function connectPreviewBridge(port: MessagePort): Promise<void> {
  const registration = await ensurePreviewServiceWorker();

  if (!registration) {
    return;
  }

  const target =
    navigator.serviceWorker.controller ??
    registration.active ??
    registration.waiting ??
    registration.installing;

  if (!target) {
    throw new Error("No Service Worker target available for preview bridge.");
  }

  postMessageToTarget(target, { type: "preview.bridge.connect" }, [port]);
}

async function registerPreviewServiceWorker(): Promise<ServiceWorkerRegistration | null> {
  try {
    const registration = await navigator.serviceWorker.register(withAppBasePath("/preview-sw.js"), {
      scope: APP_BASE_PATH,
    });

    await navigator.serviceWorker.ready;
    postMessageToTarget(
      navigator.serviceWorker.controller ??
        registration.active ??
        registration.waiting ??
        registration.installing,
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

async function postToServiceWorker(
  message: PreviewWorkerMessage | PreviewBridgeConnectMessage,
): Promise<void> {
  const registration = await ensurePreviewServiceWorker();

  if (!registration) {
    return;
  }

  const target =
    navigator.serviceWorker.controller ??
    registration.active ??
    registration.waiting ??
    registration.installing;

  if (!target) {
    queue.push(message);
    return;
  }

  postMessageToTarget(target, {
    type: "preview.configure",
    clientScriptUrl: previewClientUrl,
  });
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
  message: PreviewWorkerMessage | PreviewBridgeConnectMessage,
  transfer: Transferable[] = [],
): void {
  if (!target) {
    queue.push(message);
    return;
  }

  target.postMessage(message, transfer);
}

export function withPreviewClientHeader(headers: Record<string, string>): Record<string, string> {
  return {
    ...headers,
    [PREVIEW_CLIENT_HEADER]: previewClientUrl,
  };
}
