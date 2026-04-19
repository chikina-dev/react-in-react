const previews = new Map();
let previewClientUrl = null;

self.addEventListener("install", (event) => {
  event.waitUntil(self.skipWaiting());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("message", (event) => {
  const data = event.data;

  if (!data || typeof data !== "object") {
    return;
  }

  switch (data.type) {
    case "preview.configure":
      if (typeof data.clientScriptUrl === "string" && data.clientScriptUrl.length > 0) {
        previewClientUrl = data.clientScriptUrl;
      }
      break;
    case "preview.register":
      if (isPreviewRegistration(data.preview)) {
        previews.set(buildKey(data.preview.sessionId, data.preview.port), data.preview);
      }
      break;
    case "preview.unregister":
      if (typeof data.sessionId === "string" && typeof data.port === "number") {
        previews.delete(buildKey(data.sessionId, data.port));
      }
      break;
    case "preview.unregister-all":
      if (typeof data.sessionId === "string") {
        for (const key of previews.keys()) {
          if (key.startsWith(`${data.sessionId}:`)) {
            previews.delete(key);
          }
        }
      }
      break;
  }
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);

  if (event.request.method !== "GET") {
    return;
  }

  if (url.pathname.startsWith("/preview/")) {
    event.respondWith(handleRuntimePreviewRequest(event, url));
  }
});

async function handleRuntimePreviewRequest(event, url) {
  const previewId = extractPreviewId(url.pathname, "/preview/");

  if (!previewId) {
    return Response.json({ error: "Invalid preview URL" }, { status: 400 });
  }

  const client = await resolveBridgeClient(event.clientId);

  if (!client) {
    return Response.json(
      { error: "No bridge client available for preview request" },
      { status: 503 },
    );
  }

  const response = await requestPreviewResponseFromClient(client, {
    sessionId: previewId.sessionId,
    port: previewId.port,
    method: event.request.method,
    pathname: url.pathname,
    search: url.search,
    headers: {
      ...Object.fromEntries(event.request.headers.entries()),
      "x-react-in-react-preview-client": previewClientUrl ?? "",
    },
  });

  return new Response(response.body, {
    status: response.status,
    headers: response.headers,
  });
}

function extractPreviewId(pathname, prefix) {
  const raw = pathname.slice(prefix.length).replace(/^\/+/, "").split("/");

  if (raw.length < 2) {
    return null;
  }

  const sessionId = raw[0];
  const port = Number(raw[1]);

  if (!sessionId || Number.isNaN(port)) {
    return null;
  }

  return { sessionId, port };
}

function buildKey(sessionId, port) {
  return `${sessionId}:${port}`;
}

function isPreviewRegistration(value) {
  return (
    value &&
    typeof value === "object" &&
    typeof value.sessionId === "string" &&
    typeof value.port === "number" &&
    typeof value.url === "string" &&
    value.model &&
    typeof value.model.title === "string"
  );
}

async function resolveBridgeClient(clientId) {
  if (clientId) {
    const client = await self.clients.get(clientId);

    if (client && !new URL(client.url).pathname.startsWith("/preview/")) {
      return client;
    }
  }

  const clients = await self.clients.matchAll({
    type: "window",
    includeUncontrolled: true,
  });

  return clients.find((client) => !new URL(client.url).pathname.startsWith("/preview/")) ?? null;
}

async function requestPreviewResponseFromClient(client, request) {
  const requestId =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`;

  return await new Promise((resolve, reject) => {
    const channel = new MessageChannel();

    channel.port1.onmessage = (event) => {
      const data = event.data;

      if (!data || data.type !== "preview.http.response" || data.requestId !== requestId) {
        reject(new Error("Invalid preview bridge response"));
        return;
      }

      if (data.error) {
        reject(new Error(data.error));
        return;
      }

      resolve(data.response);
    };

    client.postMessage(
      {
        type: "preview.http.request",
        requestId,
        request,
      },
      [channel.port2],
    );
  });
}
