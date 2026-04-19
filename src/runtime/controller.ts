import type {
  PreviewHttpResponseMessage,
  RunRequest,
  RuntimeEvent,
  SessionSnapshot,
  UiToWorkerMessage,
  VirtualHttpRequest,
  VirtualHttpResponse,
  WorkerToUiMessage,
} from "./protocol";

export class RuntimeController {
  private readonly worker: Worker;
  private readonly listeners = new Set<(event: RuntimeEvent) => void>();
  private readonly pending = new Map<string, () => void>();
  private readonly pendingPreviewResponses = new Map<
    string,
    {
      resolve: (response: VirtualHttpResponse) => void;
      reject: (error: Error) => void;
    }
  >();

  constructor() {
    this.worker = new Worker(new URL("./runtime.worker.ts", import.meta.url), {
      type: "module",
    });

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

      for (const listener of this.listeners) {
        listener(message);
      }
    });
  }

  subscribe(listener: (event: RuntimeEvent) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  async createSession(file: File): Promise<SessionSnapshot> {
    const requestId = createRequestId();
    const zip = await file.arrayBuffer();

    return new Promise<SessionSnapshot>((resolve, reject) => {
      const unsubscribe = this.subscribe((event) => {
        if (event.type === "session.created" && event.requestId === requestId) {
          unsubscribe();
          resolve(event.session);
        }

        if (event.type === "runtime.error" && event.requestId === requestId) {
          unsubscribe();
          reject(new Error(event.error.message));
        }
      });

      this.postMessage({
        type: "session.create",
        requestId,
        fileName: file.name,
        zip,
      });
    });
  }

  async run(sessionId: string, request: RunRequest): Promise<void> {
    const requestId = createRequestId();
    await this.awaitAck(requestId, {
      type: "session.run",
      requestId,
      sessionId,
      request,
    });
  }

  async stop(sessionId: string): Promise<void> {
    const requestId = createRequestId();
    await this.awaitAck(requestId, {
      type: "session.stop",
      requestId,
      sessionId,
    });
  }

  async requestPreviewResponse(request: VirtualHttpRequest): Promise<VirtualHttpResponse> {
    const requestId = createRequestId();

    return new Promise<VirtualHttpResponse>((resolve, reject) => {
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

  private async awaitAck(requestId: string, message: UiToWorkerMessage): Promise<void> {
    await new Promise<void>((resolve) => {
      this.pending.set(requestId, resolve);
      this.postMessage(message);
    });
  }

  private postMessage(message: UiToWorkerMessage): void {
    if (message.type === "session.create") {
      this.worker.postMessage(message, [message.zip]);
      return;
    }

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
}

function createRequestId(): string {
  if ("randomUUID" in crypto) {
    return crypto.randomUUID();
  }

  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function decodeVirtualHttpBody(body: string | Uint8Array): string {
  if (typeof body === "string") {
    return body;
  }

  return new TextDecoder().decode(body);
}
