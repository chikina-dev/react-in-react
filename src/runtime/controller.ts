import { type MountedArchive, type WorkspaceFileRecord } from "./analyze-archive";
import type {
  PreviewReadyEvent,
  PreviewHttpResponseMessage,
  RunRequest,
  RuntimeEvent,
  SessionSnapshot,
  UiToWorkerMessage,
  VirtualHttpRequest,
  VirtualHttpResponse,
  WorkerToUiMessage,
} from "./protocol";
import {
  connectPreviewBridge,
  ensurePreviewServiceWorker,
  registerPreview,
  subscribePreviewWorkerState,
  unregisterSessionPreviews,
  type PreviewWorkerState,
} from "./preview-service-worker";
import RuntimeWorker from "./runtime.worker?worker";

const WORKER_PING_TIMEOUT_MS = 1500;
const WORKER_MOUNT_TIMEOUT_MS = 10000;
const WORKER_RUN_TIMEOUT_MS = 5000;
const WORKER_STOP_TIMEOUT_MS = 5000;

export class RuntimeController {
  private readonly worker: Worker;
  private readonly listeners = new Set<(event: RuntimeEvent) => void>();
  private readonly workerErrorListeners = new Set<(error: Error) => void>();
  private readonly pending = new Map<string, () => void>();
  private workerReadyPromise: Promise<void> | null = null;
  private lastWorkerError: Error | null = null;
  private readonly pendingPreviewResponses = new Map<
    string,
    {
      resolve: (response: VirtualHttpResponse) => void;
      reject: (error: Error) => void;
    }
  >();
  private previewBridgePromise: Promise<void> | null = null;

  constructor() {
    this.worker = new RuntimeWorker();

    this.worker.addEventListener("message", (event: MessageEvent<WorkerToUiMessage>) => {
      const message = event.data;

      if (message.type === "ack") {
        const resolve = this.pending.get(message.requestId);

        if (resolve) {
          this.pending.delete(message.requestId);
          resolve();
        }

        return;
      }

      if (message.type === "preview.http.response") {
        this.resolvePreviewResponse(message);
        return;
      }

      if (message.type === "preview.ready") {
        void this.handlePreviewReady(message);
        return;
      }

      this.emitLocalRuntimeEvent(message);
    });

    this.worker.addEventListener("error", (event) => {
      const error = new Error(event.message || "Runtime worker failed to load.");
      this.handleWorkerFailure(error);
    });

    this.worker.addEventListener("messageerror", () => {
      const error = new Error("Runtime worker could not deserialize a message.");
      this.handleWorkerFailure(error);
    });

    if (typeof window !== "undefined" && "serviceWorker" in navigator) {
      void this.ensurePreviewWorker();
    }
  }

  subscribe(listener: (event: RuntimeEvent) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  subscribeWorkerErrors(listener: (error: Error) => void): () => void {
    if (this.lastWorkerError) {
      listener(this.lastWorkerError);
    }
    this.workerErrorListeners.add(listener);
    return () => {
      this.workerErrorListeners.delete(listener);
    };
  }

  subscribePreviewWorkerState(listener: (state: PreviewWorkerState) => void): () => void {
    return subscribePreviewWorkerState(listener);
  }

  async ensurePreviewWorker(): Promise<void> {
    await ensurePreviewServiceWorker();
    await this.ensurePreviewBridge();
  }

  async createSession(file: File): Promise<SessionSnapshot> {
    const zip = await file.arrayBuffer();
    await this.ensureWorkerResponsive();
    const requestId = createRequestId();
    const sessionCreated = this.awaitSessionCreated(requestId);
    await withTimeout(
      this.awaitAck(requestId, {
        type: "session.create",
        requestId,
        fileName: file.name,
        zip,
      }),
      WORKER_MOUNT_TIMEOUT_MS,
    );
    return await withTimeout(sessionCreated, WORKER_MOUNT_TIMEOUT_MS);
  }

  async replaceSession(previousSessionId: string | null, file: File): Promise<SessionSnapshot> {
    if (previousSessionId) {
      await this.stop(previousSessionId).catch(() => undefined);
    }

    return await this.createSession(file);
  }

  async mountSession(mounted: MountedArchive): Promise<SessionSnapshot> {
    await this.ensureWorkerResponsive();
    const requestId = createRequestId();
    const sessionCreated = this.awaitSessionCreated(requestId);
    await withTimeout(
      this.awaitAck(requestId, {
        type: "session.mount",
        requestId,
        session: mounted.snapshot,
        files: [...mounted.files.values()].map(serializeWorkspaceFileRecord),
      }),
      WORKER_MOUNT_TIMEOUT_MS,
    );
    return await withTimeout(sessionCreated, WORKER_MOUNT_TIMEOUT_MS);
  }

  async run(sessionId: string, request: RunRequest): Promise<void> {
    try {
      await this.ensureWorkerResponsive();
      const requestId = createRequestId();
      await withTimeout(
        this.awaitAck(requestId, {
          type: "session.run",
          requestId,
          sessionId,
          request,
        }),
        WORKER_RUN_TIMEOUT_MS,
      );
    } catch (error) {
      this.emitWorkerOperationFailure(
        sessionId,
        "WORKER_RUN_FAILED",
        error,
        "Failed to launch runtime through worker.",
      );
    }
  }

  async stop(sessionId: string): Promise<void> {
    const requestId = createRequestId();
    try {
      await this.ensureWorkerResponsive();
      await unregisterSessionPreviews(sessionId).catch(() => undefined);
      await withTimeout(
        this.awaitAck(requestId, {
          type: "session.stop",
          requestId,
          sessionId,
        }),
        WORKER_STOP_TIMEOUT_MS,
      );
    } catch (error) {
      this.emitWorkerOperationFailure(
        sessionId,
        "WORKER_STOP_FAILED",
        error,
        "Failed to stop runtime through worker.",
      );
    }
  }

  async requestPreviewResponse(request: VirtualHttpRequest): Promise<VirtualHttpResponse> {
    if (this.lastWorkerError) {
      throw new Error(
        `Runtime worker unavailable: ${this.lastWorkerError.message || "unknown worker failure"}`,
      );
    }

    const requestId = createRequestId();

    return await new Promise<VirtualHttpResponse>((resolve, reject) => {
      this.pendingPreviewResponses.set(requestId, { resolve, reject });
      this.postMessage({
        type: "preview.http",
        requestId,
        request,
      });
    });
  }

  async requestPreviewJson<T>(request: VirtualHttpRequest): Promise<T> {
    const response = await this.requestPreviewResponse(request);

    if (response.status >= 400) {
      throw new Error(`Preview request failed with status ${response.status}.`);
    }

    return JSON.parse(decodeVirtualHttpBody(response.body)) as T;
  }

  async requestPreviewText(request: VirtualHttpRequest): Promise<string> {
    const response = await this.requestPreviewResponse(request);

    if (response.status >= 400) {
      throw new Error(`Preview request failed with status ${response.status}.`);
    }

    return decodeVirtualHttpBody(response.body);
  }

  dispose(): void {
    for (const pending of this.pendingPreviewResponses.values()) {
      pending.reject(new Error("Runtime controller disposed."));
    }

    this.worker.terminate();
    this.pending.clear();
    this.pendingPreviewResponses.clear();
    this.listeners.clear();
  }

  private async ensureWorkerResponsive(): Promise<void> {
    if (this.lastWorkerError) {
      throw new Error(this.lastWorkerError.message || "Runtime worker is unavailable.");
    }

    if (!this.workerReadyPromise) {
      const requestId = createRequestId();
      this.workerReadyPromise = this.awaitAck(requestId, {
        type: "worker.ping",
        requestId,
      }).catch((error) => {
        this.workerReadyPromise = null;
        throw error;
      });
    }

    try {
      await withTimeout(this.workerReadyPromise, WORKER_PING_TIMEOUT_MS);
    } catch (error) {
      throw new Error(
        error instanceof Error ? error.message : "Runtime worker did not become responsive.",
      );
    }
  }

  private async ensurePreviewBridge(): Promise<void> {
    if (typeof window === "undefined" || !("serviceWorker" in navigator)) {
      return;
    }

    if (!this.previewBridgePromise) {
      const channel = new MessageChannel();
      this.worker.postMessage({ type: "preview.bridge.connect" }, [channel.port1]);
      this.previewBridgePromise = connectPreviewBridge(channel.port2).catch((error) => {
        this.previewBridgePromise = null;
        throw error;
      });
    }

    await this.previewBridgePromise;
  }

  private async handlePreviewReady(event: PreviewReadyEvent): Promise<void> {
    try {
      await registerPreview(event);
      this.emitLocalRuntimeEvent(event);
    } catch (error) {
      this.emitLocalRuntimeEvent({
        type: "session.state",
        sessionId: event.sessionId,
        state: "errored",
      });
      this.emitLocalRuntimeEvent({
        type: "runtime.error",
        sessionId: event.sessionId,
        error: {
          code: "PREVIEW_REGISTER_FAILED",
          message: error instanceof Error ? error.message : "Unknown preview registration error",
        },
      });
    }
  }

  private emitLocalRuntimeEvent(event: RuntimeEvent): void {
    for (const listener of Array.from(this.listeners)) {
      listener(event);
    }
  }

  private async awaitAck(requestId: string, message: UiToWorkerMessage): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      const handleWorkerError = (error: Error) => {
        unsubscribe();
        this.workerErrorListeners.delete(handleWorkerError);
        this.pending.delete(requestId);
        reject(error);
      };
      const unsubscribe = this.subscribe((event) => {
        if (event.type === "runtime.error" && event.requestId === requestId) {
          unsubscribe();
          this.workerErrorListeners.delete(handleWorkerError);
          this.pending.delete(requestId);
          reject(new Error(event.error.message));
        }
      });
      this.workerErrorListeners.add(handleWorkerError);
      this.pending.set(requestId, () => {
        unsubscribe();
        this.workerErrorListeners.delete(handleWorkerError);
        resolve();
      });
      this.postMessage(message);
    });
  }

  private async awaitSessionCreated(requestId: string): Promise<SessionSnapshot> {
    return await new Promise<SessionSnapshot>((resolve, reject) => {
      const handleWorkerError = (error: Error) => {
        unsubscribe();
        this.workerErrorListeners.delete(handleWorkerError);
        reject(error);
      };
      const unsubscribe = this.subscribe((event) => {
        if (event.type === "runtime.error" && event.requestId === requestId) {
          unsubscribe();
          this.workerErrorListeners.delete(handleWorkerError);
          reject(new Error(event.error.message));
          return;
        }
        if (event.type === "session.created" && event.requestId === requestId) {
          unsubscribe();
          this.workerErrorListeners.delete(handleWorkerError);
          resolve(event.session);
        }
      });
      this.workerErrorListeners.add(handleWorkerError);
    });
  }

  private postMessage(message: UiToWorkerMessage): void {
    this.worker.postMessage(message);
  }

  private resolvePreviewResponse(message: PreviewHttpResponseMessage): void {
    const pending = this.pendingPreviewResponses.get(message.requestId);

    if (!pending) {
      return;
    }

    this.pendingPreviewResponses.delete(message.requestId);
    pending.resolve(message.response);
  }
  private handleWorkerFailure(error: Error): void {
    this.lastWorkerError = error;
    this.workerReadyPromise = null;
    for (const pending of this.pendingPreviewResponses.values()) {
      pending.reject(error);
    }
    this.pendingPreviewResponses.clear();
    for (const listener of Array.from(this.workerErrorListeners)) {
      listener(error);
    }
  }

  private emitWorkerOperationFailure(
    sessionId: string,
    code: string,
    error: unknown,
    fallbackMessage: string,
  ): void {
    this.emitLocalRuntimeEvent({
      type: "session.state",
      sessionId,
      state: "errored",
    });
    this.emitLocalRuntimeEvent({
      type: "runtime.error",
      sessionId,
      error: {
        code,
        message: error instanceof Error ? error.message : fallbackMessage,
      },
    });
  }
}

function createRequestId(): string {
  if ("randomUUID" in crypto) {
    return crypto.randomUUID();
  }

  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function serializeWorkspaceFileRecord(file: WorkspaceFileRecord): WorkspaceFileRecord {
  return {
    ...file,
    bytes: file.bytes.slice(),
  };
}

function decodeVirtualHttpBody(body: string | Uint8Array): string {
  if (typeof body === "string") {
    return body;
  }

  return new TextDecoder().decode(body);
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  let timerId: number | null = null;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timerId = window.setTimeout(() => {
          reject(new Error(`Timed out waiting for worker ack after ${timeoutMs}ms.`));
        }, timeoutMs);
      }),
    ]);
  } finally {
    if (timerId != null) {
      clearTimeout(timerId);
    }
  }
}
