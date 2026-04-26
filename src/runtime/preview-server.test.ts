import { expect, test } from "vite-plus/test";

import previewEmptyModuleUrl from "../preview/shims/empty-module.js?url";
import { PREVIEW_CLIENT_HEADER } from "./preview-constants";
import { buildPreviewResponse, isPreviewPath } from "./preview-server";
import type { VirtualHttpRequest } from "./protocol";

type PreviewState = NonNullable<Parameters<typeof buildPreviewResponse>[1]>;

const request: VirtualHttpRequest = {
  sessionId: "session-1",
  port: 3000,
  method: "GET",
  pathname: "/preview/session-1/3000/__runtime.json",
  search: "",
  headers: {},
};

function createPreviewState(files: PreviewState["files"] = new Map()): PreviewState {
  return {
    sessionId: "session-1",
    pid: 1000,
    port: 3000,
    url: "/preview/session-1/3000/",
    model: {
      title: "demo",
      summary: "preview",
      cwd: "/workspace",
      command: "npm run dev",
      highlights: ["react=true"],
    },
    host: {
      engineName: "null-engine",
      supportsInterrupts: true,
      supportsModuleLoader: true,
      workspaceRoot: "/workspace",
    },
    run: {
      cwd: "/workspace",
      entrypoint: "dev",
      commandLine: "npm run dev",
      envCount: 0,
      commandKind: "npm-script",
      resolvedScript: "vite",
    },
    hostFiles: {
      count: 2,
      samplePath: "/workspace/package.json",
      sampleSize: 12,
    },
    session: {
      sessionId: "session-1",
      state: "running",
      revision: 0,
      workspaceRoot: "/workspace",
      archive: {
        fileName: "guest.zip",
        fileCount: 2,
        directoryCount: 1,
        entries: [],
        rootPrefix: "demo",
      },
      packageJson: null,
      capabilities: {
        detectedReact: true,
      },
    },
    files,
  };
}

test("buildPreviewResponse does not build preview metadata locally", () => {
  const response = buildPreviewResponse(request, createPreviewState());

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/__runtime.json",
    }),
  );
});

test("buildPreviewResponse does not synthesize preview root HTML locally", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/",
      headers: {
        [PREVIEW_CLIENT_HEADER]: "/assets/preview-client.js",
      },
    },
    createPreviewState(),
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/",
    }),
  );
});

test("buildPreviewResponse does not synthesize app shell from root hints locally", () => {
  const state = createPreviewState(
    new Map([
      [
        "/workspace/src/main.tsx",
        {
          path: "/workspace/src/main.tsx",
          size: 24,
          contentType: "text/plain; charset=utf-8",
          isText: true,
          bytes: new TextEncoder().encode("console.log('from-hint')"),
          textContent: "console.log('from-hint')",
        },
      ],
    ]),
  );

  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/",
    },
    {
      ...state,
      rootRequestHint: {
        kind: "root-entry",
        workspacePath: "/workspace/src/main.tsx",
        documentRoot: null,
        hydratePaths: ["/workspace/src/main.tsx"],
      },
      rootResponseDescriptor: {
        kind: "app-shell",
        workspacePath: "/workspace/src/main.tsx",
        documentRoot: null,
        hydratePaths: ["/workspace/src/main.tsx"],
        statusCode: 200,
        contentType: "text/html; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/",
    }),
  );
});

test("buildPreviewResponse does not render document roots from request hints locally", () => {
  const state = createPreviewState(
    new Map([
      [
        "/workspace/dist/index.html",
        {
          path: "/workspace/dist/index.html",
          size: 51,
          contentType: "text/html; charset=utf-8",
          isText: true,
          bytes: new TextEncoder().encode('<script type="module" src="/assets/app.js"></script>'),
          textContent: '<script type="module" src="/assets/app.js"></script>',
        },
      ],
    ]),
  );

  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/",
    },
    {
      ...state,
      requestHint: {
        kind: "root-document",
        workspacePath: "/workspace/dist/index.html",
        documentRoot: "/workspace/dist",
        hydratePaths: ["/workspace/dist/index.html"],
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/",
    }),
  );
});

test("buildPreviewResponse does not render document roots from response descriptors locally", () => {
  const state = createPreviewState(
    new Map([
      [
        "/workspace/dist/index.html",
        {
          path: "/workspace/dist/index.html",
          size: 51,
          contentType: "text/html; charset=utf-8",
          isText: true,
          bytes: new TextEncoder().encode('<script type="module" src="/assets/app.js"></script>'),
          textContent: '<script type="module" src="/assets/app.js"></script>',
        },
      ],
    ]),
  );

  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/",
    },
    {
      ...state,
      responseDescriptor: {
        kind: "workspace-document",
        workspacePath: "/workspace/dist/index.html",
        documentRoot: "/workspace/dist",
        hydratePaths: ["/workspace/dist/index.html"],
        statusCode: 200,
        contentType: "text/html; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/",
    }),
  );
});

test("buildPreviewResponse respects host-provided HEAD descriptors without hydrating body", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      method: "HEAD",
      pathname: "/preview/session-1/3000/files/src/main.tsx",
    },
    {
      ...createPreviewState(),
      responseDescriptor: {
        kind: "workspace-file",
        workspacePath: "/workspace/src/main.tsx",
        documentRoot: "/workspace",
        hydratePaths: [],
        statusCode: 200,
        contentType: "text/javascript; charset=utf-8",
        allowMethods: [],
        omitBody: true,
      },
    },
  );

  expect(response.status).toBe(200);
  expect(response.headers["content-type"]).toContain("text/javascript");
  expect(response.body).toBe("");
});

test("buildPreviewResponse returns 405 from host-provided method-not-allowed descriptors", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      method: "POST",
      pathname: "/preview/session-1/3000/files/src/main.tsx",
    },
    {
      ...createPreviewState(),
      responseDescriptor: {
        kind: "method-not-allowed",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
        statusCode: 405,
        contentType: "application/json; charset=utf-8",
        allowMethods: ["GET", "HEAD"],
        omitBody: false,
      },
    },
  );

  expect(response.status).toBe(405);
  expect(response.headers.allow).toBe("GET, HEAD");
  expect(response.body).toContain('"error":"Method not allowed"');
});

test("buildPreviewResponse serves workspace files from host-provided request hints", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/files/src/main.tsx",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/src/main.tsx",
            {
              path: "/workspace/src/main.tsx",
              size: 18,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("console.log('hint')"),
              textContent: "console.log('hint')",
            },
          ],
        ]),
      ),
      requestHint: {
        kind: "workspace-file",
        workspacePath: "/workspace/src/main.tsx",
        documentRoot: "/workspace",
        hydratePaths: ["/workspace/src/main.tsx"],
      },
    },
  );

  expect(response.status).toBe(200);
  expect(response.headers["content-type"]).toContain("text/javascript");
  expect(response.body).toContain("console.log('hint')");
});

test("buildPreviewResponse returns 404 from host-provided not-found request hints", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/missing.js",
    },
    {
      ...createPreviewState(),
      requestHint: {
        kind: "not-found",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toContain('"error":"Unsupported preview path"');
});

test("buildPreviewResponse does not build runtime stylesheet locally", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/assets/runtime.css",
    },
    createPreviewState(),
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/assets/runtime.css",
    }),
  );
});

test("buildPreviewResponse does not build preview file index locally", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/__files.json",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/package.json",
          {
            path: "/workspace/package.json",
            size: 12,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new Uint8Array([123, 34, 110, 97, 109, 101, 34, 58, 34, 120, 34, 125]),
            textContent: '{"name":"x"}',
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/__files.json",
    }),
  );
});

test("buildPreviewResponse does not build preview diagnostics locally", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/__diagnostics.json",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/package.json",
            {
              path: "/workspace/package.json",
              size: 12,
              contentType: "application/json; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode('{"name":"x"}'),
              textContent: '{"name":"x"}',
            },
          ],
        ]),
      ),
      rootRequestHint: {
        kind: "fallback-root",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      },
      requestHint: {
        kind: "diagnostics-state",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/__diagnostics.json",
    }),
  );
});

test("buildPreviewResponse returns workspace file contents", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/files/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 18,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("console.log('hi')"),
            textContent: "console.log('hi')",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(response.headers["content-type"]).toContain("text/javascript");
  expect(response.body).toContain("console.log");
});

test("buildPreviewResponse does not serve workspace index.html locally", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/index.html",
          {
            path: "/workspace/index.html",
            size: 72,
            contentType: "text/html; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '<!doctype html><html><body><script src="/assets/app.js"></script></body></html>',
            ),
            textContent:
              '<!doctype html><html><body><script src="/assets/app.js"></script></body></html>',
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/",
    }),
  );
});

test("buildPreviewResponse does not synthesize app shell from source entry locally", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 68,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("import App from './App.tsx';\nconsole.log(App);"),
            textContent: "import App from './App.tsx';\nconsole.log(App);",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/",
    }),
  );
});

test("buildPreviewResponse does not infer document-root assets locally", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/assets/app.js",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/dist/index.html",
          {
            path: "/workspace/dist/index.html",
            size: 40,
            contentType: "text/html; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('<script type="module" src="/assets/app.js"></script>'),
            textContent: '<script type="module" src="/assets/app.js"></script>',
          },
        ],
        [
          "/workspace/dist/assets/app.js",
          {
            path: "/workspace/dist/assets/app.js",
            size: 21,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("console.log('dist');"),
            textContent: "console.log('dist');",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/assets/app.js",
    }),
  );
});

test("buildPreviewResponse prefers host-provided asset hints for document-root assets", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/assets/app.js",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/dist/assets/app.js",
            {
              path: "/workspace/dist/assets/app.js",
              size: 21,
              contentType: "text/javascript; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("console.log('hint');"),
              textContent: "console.log('hint');",
            },
          ],
        ]),
      ),
      requestHint: {
        kind: "workspace-asset",
        workspacePath: "/workspace/dist/assets/app.js",
        documentRoot: "/workspace/dist",
        hydratePaths: ["/workspace/dist/assets/app.js"],
      },
    },
  );

  expect(response.status).toBe(200);
  expect(response.headers["content-type"]).toContain("text/javascript");
  expect(response.body).toContain("console.log('hint')");
});

test("buildPreviewResponse does not rewrite stylesheet bodies locally", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/styles/site.css",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/styles/site.css",
          {
            path: "/workspace/styles/site.css",
            size: 32,
            contentType: "text/css; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("body{background:url('/hero.png')}"),
            textContent: "body{background:url('/hero.png')}",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/styles/site.css",
    }),
  );
});

test("buildPreviewResponse does not return binary assets locally", () => {
  const bytes = new Uint8Array([137, 80, 78, 71]);
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/assets/logo.png",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/index.html",
          {
            path: "/workspace/index.html",
            size: 58,
            contentType: "text/html; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('<img src="/assets/logo.png" alt="logo" />'),
            textContent: '<img src="/assets/logo.png" alt="logo" />',
          },
        ],
        [
          "/workspace/assets/logo.png",
          {
            path: "/workspace/assets/logo.png",
            size: bytes.byteLength,
            contentType: "image/png",
            isText: false,
            bytes,
            textContent: null,
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/assets/logo.png",
    }),
  );
});

test("buildPreviewResponse transpiles TSX modules and rewrites React and CSS imports", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/index.html",
          {
            path: "/workspace/index.html",
            size: 71,
            contentType: "text/html; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '<!doctype html><html><body><script type="module" src="/src/main.tsx"></script>',
            ),
            textContent:
              '<!doctype html><html><body><script type="module" src="/src/main.tsx"></script>',
          },
        ],
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 171,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              [
                'import React from "react";',
                'import { createRoot } from "react-dom/client";',
                'import "./index.css";',
                'import App from "/src/App.tsx";',
                "createRoot(document.getElementById('root')!).render(<App />);",
              ].join("\n"),
            ),
            textContent: [
              'import React from "react";',
              'import { createRoot } from "react-dom/client";',
              'import "./index.css";',
              'import App from "/src/App.tsx";',
              "createRoot(document.getElementById('root')!).render(<App />);",
            ].join("\n"),
          },
        ],
        [
          "/workspace/src/App.tsx",
          {
            path: "/workspace/src/App.tsx",
            size: 25,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default function App() { return null; }"),
            textContent: "export default function App() { return null; }",
          },
        ],
        [
          "/workspace/src/index.css",
          {
            path: "/workspace/src/index.css",
            size: 18,
            contentType: "text/css; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("body { color: red; }"),
            textContent: "body { color: red; }",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(response.headers["content-type"]).toContain("text/javascript");
  expect(typeof response.body).toBe("string");
  expect(response.body).not.toContain('from "react"');
  expect(response.body).not.toContain('from "react-dom/client"');
  expect(response.body).toContain("__nodeInNodeAttachStylesheet");
  expect(response.body).toContain("/preview/session-1/3000/src/App.tsx");
  expect(response.body).toContain("/preview/session-1/3000/src/index.css");
});

test("buildPreviewResponse resolves extensionless relative ESM imports", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 93,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import App from "./App";\nimport { mount } from "./boot/index";\nconsole.log(App, mount);',
            ),
            textContent:
              'import App from "./App";\nimport { mount } from "./boot/index";\nconsole.log(App, mount);',
          },
        ],
        [
          "/workspace/src/App.tsx",
          {
            path: "/workspace/src/App.tsx",
            size: 18,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'App';"),
            textContent: "export default 'App';",
          },
        ],
        [
          "/workspace/src/boot/index.ts",
          {
            path: "/workspace/src/boot/index.ts",
            size: 21,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export const mount = 1;"),
            textContent: "export const mount = 1;",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/src/App.tsx");
  expect(response.body).toContain("/preview/session-1/3000/src/boot/index.ts");
});

test("buildPreviewResponse resolves simple node_modules package entrypoints", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 55,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('import demo from "demo-lib";\nconsole.log(demo);'),
            textContent: 'import demo from "demo-lib";\nconsole.log(demo);',
          },
        ],
        [
          "/workspace/node_modules/demo-lib/package.json",
          {
            path: "/workspace/node_modules/demo-lib/package.json",
            size: 36,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"main":"dist/index.js"}'),
            textContent: '{"main":"dist/index.js"}',
          },
        ],
        [
          "/workspace/node_modules/demo-lib/dist/index.js",
          {
            path: "/workspace/node_modules/demo-lib/dist/index.js",
            size: 22,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'demo';"),
            textContent: "export default 'demo';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/node_modules/demo-lib/dist/index.js");
});

test("buildPreviewResponse prefers backend module plans over local package resolution", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/src/main.tsx",
            {
              path: "/workspace/src/main.tsx",
              size: 55,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode('import demo from "demo-lib";\nconsole.log(demo);'),
              textContent: 'import demo from "demo-lib";\nconsole.log(demo);',
            },
          ],
          [
            "/workspace/node_modules/demo-lib/dist/index.js",
            {
              path: "/workspace/node_modules/demo-lib/dist/index.js",
              size: 22,
              contentType: "text/javascript; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("export default 'demo';"),
              textContent: "export default 'demo';",
            },
          ],
        ]),
      ),
      modulePlan: {
        importerPath: "/workspace/src/main.tsx",
        format: "module",
        importPlans: [
          {
            requestSpecifier: "demo-lib",
            previewSpecifier: "/preview/session-1/3000/node_modules/demo-lib/dist/index.js",
            format: "module",
          },
        ],
      },
    },
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/node_modules/demo-lib/dist/index.js");
});

test("buildPreviewResponse does not fall back to local package resolution when backend module plan is present", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/src/main.tsx",
            {
              path: "/workspace/src/main.tsx",
              size: 61,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode(
                'import demo from "browser-lib";\nconsole.log(demo);',
              ),
              textContent: 'import demo from "browser-lib";\nconsole.log(demo);',
            },
          ],
          [
            "/workspace/node_modules/browser-lib/package.json",
            {
              path: "/workspace/node_modules/browser-lib/package.json",
              size: 65,
              contentType: "application/json; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode(
                '{"browser":"./browser/index.js","main":"./server/index.js"}',
              ),
              textContent: '{"browser":"./browser/index.js","main":"./server/index.js"}',
            },
          ],
          [
            "/workspace/node_modules/browser-lib/browser/index.js",
            {
              path: "/workspace/node_modules/browser-lib/browser/index.js",
              size: 27,
              contentType: "text/javascript; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("export default 'browser';"),
              textContent: "export default 'browser';",
            },
          ],
        ]),
      ),
      modulePlan: {
        importerPath: "/workspace/src/main.tsx",
        format: "module",
        importPlans: [],
      },
    },
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain('from "browser-lib"');
  expect(response.body).not.toContain("/preview/session-1/3000/node_modules/browser-lib/");
});

test("buildPreviewResponse does not rewrite imports from local resolver when backend-owned request lacks module plan", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/generated/entry",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/generated/entry",
            {
              path: "/workspace/generated/entry",
              size: 84,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode(
                'import demo from "demo-lib";\nimport local from "./local";\nconsole.log(demo, local);',
              ),
              textContent:
                'import demo from "demo-lib";\nimport local from "./local";\nconsole.log(demo, local);',
            },
          ],
          [
            "/workspace/generated/local.ts",
            {
              path: "/workspace/generated/local.ts",
              size: 21,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("export default 'local';"),
              textContent: "export default 'local';",
            },
          ],
          [
            "/workspace/node_modules/demo-lib/package.json",
            {
              path: "/workspace/node_modules/demo-lib/package.json",
              size: 36,
              contentType: "application/json; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode('{"main":"dist/index.js"}'),
              textContent: '{"main":"dist/index.js"}',
            },
          ],
          [
            "/workspace/node_modules/demo-lib/dist/index.js",
            {
              path: "/workspace/node_modules/demo-lib/dist/index.js",
              size: 22,
              contentType: "text/javascript; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("export default 'demo';"),
              textContent: "export default 'demo';",
            },
          ],
        ]),
      ),
      requestHint: {
        kind: "workspace-asset",
        workspacePath: "/workspace/generated/entry",
        documentRoot: "/workspace",
        hydratePaths: ["/workspace/generated/entry"],
      },
      responseDescriptor: {
        kind: "workspace-asset",
        workspacePath: "/workspace/generated/entry",
        documentRoot: "/workspace",
        hydratePaths: ["/workspace/generated/entry"],
        statusCode: 200,
        contentType: "text/plain; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      },
      transformKind: "module",
      renderPlan: {
        kind: "workspace-file",
        workspacePath: "/workspace/generated/entry",
        documentRoot: "/workspace",
      },
    },
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain('from "demo-lib"');
  expect(response.body).toContain('from "./local"');
  expect(response.body).not.toContain("/preview/session-1/3000/node_modules/demo-lib/");
  expect(response.body).not.toContain("/preview/session-1/3000/generated/local");
});

test("buildPreviewResponse rewrites static asset imports to preview URLs for backend-owned modules", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/App.tsx",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/src/App.tsx",
            {
              path: "/workspace/src/App.tsx",
              size: 59,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode(
                'import heroImg from "./assets/hero.png";\nexport default heroImg;\n',
              ),
              textContent: 'import heroImg from "./assets/hero.png";\nexport default heroImg;\n',
            },
          ],
          [
            "/workspace/src/assets/hero.png",
            {
              path: "/workspace/src/assets/hero.png",
              size: 4,
              contentType: "image/png",
              isText: false,
              bytes: new Uint8Array([137, 80, 78, 71]),
              textContent: null,
            },
          ],
        ]),
      ),
      requestHint: {
        kind: "workspace-file",
        workspacePath: "/workspace/src/App.tsx",
        documentRoot: "/workspace",
        hydratePaths: ["/workspace/src/App.tsx"],
      },
      responseDescriptor: {
        kind: "workspace-file",
        workspacePath: "/workspace/src/App.tsx",
        documentRoot: "/workspace",
        hydratePaths: ["/workspace/src/App.tsx"],
        statusCode: 200,
        contentType: "text/javascript; charset=utf-8",
        allowMethods: ["GET", "HEAD"],
        omitBody: false,
      },
      transformKind: "module",
      modulePlan: {
        importerPath: "/workspace/src/App.tsx",
        format: "module",
        importPlans: [],
      },
    },
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain('const heroImg = "/preview/session-1/3000/src/assets/hero.png";');
  expect(response.body).not.toContain('from "./assets/hero.png"');
});

test("buildPreviewResponse uses backend transform kind for extensionless module requests", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/generated/entry",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/generated/entry",
            {
              path: "/workspace/generated/entry",
              size: 58,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode('import demo from "demo-lib";\nconsole.log(demo);'),
              textContent: 'import demo from "demo-lib";\nconsole.log(demo);',
            },
          ],
          [
            "/workspace/node_modules/demo-lib/dist/index.js",
            {
              path: "/workspace/node_modules/demo-lib/dist/index.js",
              size: 22,
              contentType: "text/javascript; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("export default 'demo';"),
              textContent: "export default 'demo';",
            },
          ],
        ]),
      ),
      requestHint: {
        kind: "workspace-asset",
        workspacePath: "/workspace/generated/entry",
        documentRoot: "/workspace",
        hydratePaths: ["/workspace/generated/entry"],
      },
      responseDescriptor: {
        kind: "workspace-asset",
        workspacePath: "/workspace/generated/entry",
        documentRoot: "/workspace",
        hydratePaths: ["/workspace/generated/entry"],
        statusCode: 200,
        contentType: "text/plain; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      },
      transformKind: "module",
      modulePlan: {
        importerPath: "/workspace/generated/entry",
        format: "module",
        importPlans: [
          {
            requestSpecifier: "demo-lib",
            previewSpecifier: "/preview/session-1/3000/node_modules/demo-lib/dist/index.js",
            format: "module",
          },
        ],
      },
    },
  );

  expect(response.status).toBe(200);
  expect(response.headers["content-type"]).toBe("text/javascript; charset=utf-8");
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/node_modules/demo-lib/dist/index.js");
});

test("buildPreviewResponse executes backend render plan before local descriptor routing", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/generated/entry",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/generated/entry",
            {
              path: "/workspace/generated/entry",
              size: 21,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("console.log('render');"),
              textContent: "console.log('render');",
            },
          ],
        ]),
      ),
      renderPlan: {
        kind: "workspace-file",
        workspacePath: "/workspace/generated/entry",
        documentRoot: "/workspace",
      },
      requestHint: {
        kind: "not-found",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      },
      responseDescriptor: {
        kind: "workspace-asset",
        workspacePath: "/workspace/generated/entry",
        documentRoot: "/workspace",
        hydratePaths: ["/workspace/generated/entry"],
        statusCode: 200,
        contentType: "text/plain; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      },
      transformKind: "module",
      modulePlan: {
        importerPath: "/workspace/generated/entry",
        format: "module",
        importPlans: [],
      },
    },
  );

  expect(response.status).toBe(200);
  expect(response.headers["content-type"]).toBe("text/javascript; charset=utf-8");
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("console.log('render');");
});

test("buildPreviewResponse does not fall back to URL-derived files when backend render plan points elsewhere", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/actual.ts",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/src/actual.ts",
            {
              path: "/workspace/src/actual.ts",
              size: 20,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("console.log('actual')"),
              textContent: "console.log('actual')",
            },
          ],
        ]),
      ),
      renderPlan: {
        kind: "workspace-file",
        workspacePath: "/workspace/src/missing.ts",
        documentRoot: "/workspace",
      },
      requestHint: {
        kind: "workspace-asset",
        workspacePath: "/workspace/src/missing.ts",
        documentRoot: "/workspace",
        hydratePaths: [],
      },
      responseDescriptor: {
        kind: "workspace-asset",
        workspacePath: "/workspace/src/missing.ts",
        documentRoot: "/workspace",
        hydratePaths: [],
        statusCode: 200,
        contentType: "text/plain; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      },
      transformKind: "module",
      modulePlan: {
        importerPath: "/workspace/src/missing.ts",
        format: "module",
        importPlans: [],
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/src/actual.ts",
    }),
  );
});

test("buildPreviewResponse does not fall back to local root routing when backend preview state is present", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/index.html",
            {
              path: "/workspace/index.html",
              size: 58,
              contentType: "text/html; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode('<img src="/assets/logo.png" alt="logo" />'),
              textContent: '<img src="/assets/logo.png" alt="logo" />',
            },
          ],
        ]),
      ),
      rootRequestHint: {
        kind: "not-found",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/",
    }),
  );
});

test("buildPreviewResponse does not fall back to local workspace files when backend preview state is present", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/files/src/main.tsx",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/src/main.tsx",
            {
              path: "/workspace/src/main.tsx",
              size: 18,
              contentType: "text/plain; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode("console.log('hi')"),
              textContent: "console.log('hi')",
            },
          ],
        ]),
      ),
      rootRequestHint: {
        kind: "fallback-root",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/files/src/main.tsx",
    }),
  );
});

test("buildPreviewResponse does not fall back to local assets when backend preview state is present", () => {
  const bytes = new Uint8Array([137, 80, 78, 71]);
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/assets/logo.png",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/assets/logo.png",
            {
              path: "/workspace/assets/logo.png",
              size: bytes.byteLength,
              contentType: "image/png",
              isText: false,
              bytes,
              textContent: null,
            },
          ],
        ]),
      ),
      rootRequestHint: {
        kind: "fallback-root",
        workspacePath: null,
        documentRoot: null,
        hydratePaths: [],
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/assets/logo.png",
    }),
  );
});

test("buildPreviewResponse does not fall back to URL-derived assets when backend descriptor points elsewhere", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/images/logo.svg",
    },
    {
      ...createPreviewState(
        new Map([
          [
            "/workspace/images/logo.svg",
            {
              path: "/workspace/images/logo.svg",
              size: 18,
              contentType: "image/svg+xml; charset=utf-8",
              isText: true,
              bytes: new TextEncoder().encode('<svg href="/x" />'),
              textContent: '<svg href="/x" />',
            },
          ],
        ]),
      ),
      requestHint: {
        kind: "workspace-asset",
        workspacePath: "/workspace/images/missing.svg",
        documentRoot: "/workspace",
        hydratePaths: [],
      },
      responseDescriptor: {
        kind: "workspace-asset",
        workspacePath: "/workspace/images/missing.svg",
        documentRoot: "/workspace",
        hydratePaths: [],
        statusCode: 200,
        contentType: "image/svg+xml; charset=utf-8",
        allowMethods: [],
        omitBody: false,
      },
      transformKind: "svg-document",
      renderPlan: {
        kind: "workspace-file",
        workspacePath: "/workspace/images/missing.svg",
        documentRoot: "/workspace",
      },
    },
  );

  expect(response.status).toBe(404);
  expect(response.body).toEqual(
    JSON.stringify({
      error: "Unsupported preview path",
      pathname: "/preview/session-1/3000/images/logo.svg",
    }),
  );
});

test("buildPreviewResponse prefers browser field for legacy package entrypoints", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 59,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('import demo from "browser-lib";\nconsole.log(demo);'),
            textContent: 'import demo from "browser-lib";\nconsole.log(demo);',
          },
        ],
        [
          "/workspace/node_modules/browser-lib/package.json",
          {
            path: "/workspace/node_modules/browser-lib/package.json",
            size: 65,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"browser":"./browser/index.js","main":"./server/index.js"}',
            ),
            textContent: '{"browser":"./browser/index.js","main":"./server/index.js"}',
          },
        ],
        [
          "/workspace/node_modules/browser-lib/browser/index.js",
          {
            path: "/workspace/node_modules/browser-lib/browser/index.js",
            size: 27,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'browser';"),
            textContent: "export default 'browser';",
          },
        ],
        [
          "/workspace/node_modules/browser-lib/server/index.js",
          {
            path: "/workspace/node_modules/browser-lib/server/index.js",
            size: 26,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'server';"),
            textContent: "export default 'server';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/browser-lib/browser/index.js",
  );
  expect(response.body).not.toContain(
    "/preview/session-1/3000/node_modules/browser-lib/server/index.js",
  );
});

test("buildPreviewResponse resolves browser object subpath mappings", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 66,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import worker from "browser-map-lib/worker";\nconsole.log(worker);',
            ),
            textContent: 'import worker from "browser-map-lib/worker";\nconsole.log(worker);',
          },
        ],
        [
          "/workspace/node_modules/browser-map-lib/package.json",
          {
            path: "/workspace/node_modules/browser-map-lib/package.json",
            size: 63,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"browser":{"./worker.js":"./browser/worker.js"}}'),
            textContent: '{"browser":{"./worker.js":"./browser/worker.js"}}',
          },
        ],
        [
          "/workspace/node_modules/browser-map-lib/browser/worker.js",
          {
            path: "/workspace/node_modules/browser-map-lib/browser/worker.js",
            size: 26,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'worker';"),
            textContent: "export default 'worker';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/browser-map-lib/browser/worker.js",
  );
});

test("buildPreviewResponse stubs browser object false mappings with an empty module", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 66,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import worker from "browser-map-lib/worker";\nconsole.log(worker);',
            ),
            textContent: 'import worker from "browser-map-lib/worker";\nconsole.log(worker);',
          },
        ],
        [
          "/workspace/node_modules/browser-map-lib/package.json",
          {
            path: "/workspace/node_modules/browser-map-lib/package.json",
            size: 34,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"browser":{"./worker.js":false}}'),
            textContent: '{"browser":{"./worker.js":false}}',
          },
        ],
        [
          "/workspace/node_modules/browser-map-lib/worker.js",
          {
            path: "/workspace/node_modules/browser-map-lib/worker.js",
            size: 33,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'server-worker';"),
            textContent: "export default 'server-worker';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(previewEmptyModuleUrl);
  expect(response.body).not.toContain(
    "/preview/session-1/3000/node_modules/browser-map-lib/worker.js",
  );
});

test("buildPreviewResponse resolves package exports root entrypoints", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 56,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('import demo from "exports-lib";\nconsole.log(demo);'),
            textContent: 'import demo from "exports-lib";\nconsole.log(demo);',
          },
        ],
        [
          "/workspace/node_modules/exports-lib/package.json",
          {
            path: "/workspace/node_modules/exports-lib/package.json",
            size: 78,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"exports":{"import":"./esm/index.js","default":"./cjs/index.cjs"}}',
            ),
            textContent: '{"exports":{"import":"./esm/index.js","default":"./cjs/index.cjs"}}',
          },
        ],
        [
          "/workspace/node_modules/exports-lib/esm/index.js",
          {
            path: "/workspace/node_modules/exports-lib/esm/index.js",
            size: 25,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'exports';"),
            textContent: "export default 'exports';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/node_modules/exports-lib/esm/index.js");
});

test("buildPreviewResponse prefers nested browser conditions in exports", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 60,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('import demo from "cond-lib";\nconsole.log(demo);'),
            textContent: 'import demo from "cond-lib";\nconsole.log(demo);',
          },
        ],
        [
          "/workspace/node_modules/cond-lib/package.json",
          {
            path: "/workspace/node_modules/cond-lib/package.json",
            size: 107,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"exports":{".":{"browser":{"import":"./browser/index.js"},"default":"./server/index.js"}}}',
            ),
            textContent:
              '{"exports":{".":{"browser":{"import":"./browser/index.js"},"default":"./server/index.js"}}}',
          },
        ],
        [
          "/workspace/node_modules/cond-lib/browser/index.js",
          {
            path: "/workspace/node_modules/cond-lib/browser/index.js",
            size: 28,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'browser';"),
            textContent: "export default 'browser';",
          },
        ],
        [
          "/workspace/node_modules/cond-lib/server/index.js",
          {
            path: "/workspace/node_modules/cond-lib/server/index.js",
            size: 27,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'server';"),
            textContent: "export default 'server';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/node_modules/cond-lib/browser/index.js");
  expect(response.body).not.toContain(
    "/preview/session-1/3000/node_modules/cond-lib/server/index.js",
  );
});

test("buildPreviewResponse resolves package exports subpaths", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 65,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import button from "exports-lib/button";\nconsole.log(button);',
            ),
            textContent: 'import button from "exports-lib/button";\nconsole.log(button);',
          },
        ],
        [
          "/workspace/node_modules/exports-lib/package.json",
          {
            path: "/workspace/node_modules/exports-lib/package.json",
            size: 73,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"exports":{".":"./esm/index.js","./button":"./esm/button.js"}}',
            ),
            textContent: '{"exports":{".":"./esm/index.js","./button":"./esm/button.js"}}',
          },
        ],
        [
          "/workspace/node_modules/exports-lib/esm/button.js",
          {
            path: "/workspace/node_modules/exports-lib/esm/button.js",
            size: 24,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'button';"),
            textContent: "export default 'button';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/node_modules/exports-lib/esm/button.js");
});

test("buildPreviewResponse resolves package exports wildcard subpaths", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 72,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import card from "wild-lib/components/card";\nconsole.log(card);',
            ),
            textContent: 'import card from "wild-lib/components/card";\nconsole.log(card);',
          },
        ],
        [
          "/workspace/node_modules/wild-lib/package.json",
          {
            path: "/workspace/node_modules/wild-lib/package.json",
            size: 50,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"exports":{"./components/*":"./esm/components/*.js"}}',
            ),
            textContent: '{"exports":{"./components/*":"./esm/components/*.js"}}',
          },
        ],
        [
          "/workspace/node_modules/wild-lib/esm/components/card.js",
          {
            path: "/workspace/node_modules/wild-lib/esm/components/card.js",
            size: 22,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'card';"),
            textContent: "export default 'card';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/wild-lib/esm/components/card.js",
  );
});

test("buildPreviewResponse rewrites basic CommonJS modules into browser ESM", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/entry.cjs",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/entry.cjs",
          {
            path: "/workspace/src/entry.cjs",
            size: 287,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              [
                'const React = require("react");',
                'const { createRoot } = require("react-dom/client");',
                'require("./entry.css");',
                'exports.mount = () => createRoot(document.getElementById("root")).render(React.createElement("div", null, "hi"));',
                "module.exports = { mount: exports.mount };",
              ].join("\n"),
            ),
            textContent: [
              'const React = require("react");',
              'const { createRoot } = require("react-dom/client");',
              'require("./entry.css");',
              'exports.mount = () => createRoot(document.getElementById("root")).render(React.createElement("div", null, "hi"));',
              "module.exports = { mount: exports.mount };",
            ].join("\n"),
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(response.headers["content-type"]).toContain("text/javascript");
  expect(typeof response.body).toBe("string");
  expect(response.body).not.toContain("require(");
  expect(response.body).toContain("__nodeInNodeCjsInterop");
  expect(response.body).toContain("__nodeInNodeAttachStylesheet");
  expect(response.body).toContain("export { __nodeInNodeExport0 as mount }");
  expect(response.body).toContain("export default __nodeInNodeDefaultExport;");
});

test("buildPreviewResponse applies browser mappings to relative ESM imports inside node_modules", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/browser-rel-lib/index.js",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/browser-rel-lib/package.json",
          {
            path: "/workspace/node_modules/browser-rel-lib/package.json",
            size: 61,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"browser":{"./server.js":"./browser.js"}}'),
            textContent: '{"browser":{"./server.js":"./browser.js"}}',
          },
        ],
        [
          "/workspace/node_modules/browser-rel-lib/index.js",
          {
            path: "/workspace/node_modules/browser-rel-lib/index.js",
            size: 56,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import server from "./server.js";\nexport default server;',
            ),
            textContent: 'import server from "./server.js";\nexport default server;',
          },
        ],
        [
          "/workspace/node_modules/browser-rel-lib/server.js",
          {
            path: "/workspace/node_modules/browser-rel-lib/server.js",
            size: 26,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'server';"),
            textContent: "export default 'server';",
          },
        ],
        [
          "/workspace/node_modules/browser-rel-lib/browser.js",
          {
            path: "/workspace/node_modules/browser-rel-lib/browser.js",
            size: 27,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'browser';"),
            textContent: "export default 'browser';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/browser-rel-lib/browser.js",
  );
  expect(response.body).not.toContain(
    "/preview/session-1/3000/node_modules/browser-rel-lib/server.js",
  );
});

test("buildPreviewResponse applies browser false mappings to relative CommonJS requires inside node_modules", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/browser-rel-cjs/index.cjs",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/browser-rel-cjs/package.json",
          {
            path: "/workspace/node_modules/browser-rel-cjs/package.json",
            size: 34,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"browser":{"./server.js":false}}'),
            textContent: '{"browser":{"./server.js":false}}',
          },
        ],
        [
          "/workspace/node_modules/browser-rel-cjs/index.cjs",
          {
            path: "/workspace/node_modules/browser-rel-cjs/index.cjs",
            size: 68,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'const server = require("./server");\nmodule.exports = server;',
            ),
            textContent: 'const server = require("./server");\nmodule.exports = server;',
          },
        ],
        [
          "/workspace/node_modules/browser-rel-cjs/server.js",
          {
            path: "/workspace/node_modules/browser-rel-cjs/server.js",
            size: 26,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'server';"),
            textContent: "export default 'server';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).not.toContain('require("./server")');
  expect(response.body).toContain(previewEmptyModuleUrl);
  expect(response.body).not.toContain(
    "/preview/session-1/3000/node_modules/browser-rel-cjs/server.js",
  );
});

test("buildPreviewResponse resolves self imports inside node_modules through the package manifest name", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/self-lib/index.js",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/self-lib/package.json",
          {
            path: "/workspace/node_modules/self-lib/package.json",
            size: 92,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"name":"self-lib","exports":{".":"./index.js","./button":"./browser/button.js"}}',
            ),
            textContent:
              '{"name":"self-lib","exports":{".":"./index.js","./button":"./browser/button.js"}}',
          },
        ],
        [
          "/workspace/node_modules/self-lib/index.js",
          {
            path: "/workspace/node_modules/self-lib/index.js",
            size: 60,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import button from "self-lib/button";\nexport default button;',
            ),
            textContent: 'import button from "self-lib/button";\nexport default button;',
          },
        ],
        [
          "/workspace/node_modules/self-lib/browser/button.js",
          {
            path: "/workspace/node_modules/self-lib/browser/button.js",
            size: 27,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'button';"),
            textContent: "export default 'button';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/self-lib/browser/button.js",
  );
});

test("buildPreviewResponse applies self browser false mappings inside node_modules", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/self-cjs/index.cjs",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/self-cjs/package.json",
          {
            path: "/workspace/node_modules/self-cjs/package.json",
            size: 54,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"name":"self-cjs","browser":{"./server.js":false}}'),
            textContent: '{"name":"self-cjs","browser":{"./server.js":false}}',
          },
        ],
        [
          "/workspace/node_modules/self-cjs/index.cjs",
          {
            path: "/workspace/node_modules/self-cjs/index.cjs",
            size: 64,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'const server = require("self-cjs/server");\nmodule.exports = server;',
            ),
            textContent: 'const server = require("self-cjs/server");\nmodule.exports = server;',
          },
        ],
        [
          "/workspace/node_modules/self-cjs/server.js",
          {
            path: "/workspace/node_modules/self-cjs/server.js",
            size: 26,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'server';"),
            textContent: "export default 'server';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).not.toContain('require("self-cjs/server")');
  expect(response.body).toContain(previewEmptyModuleUrl);
  expect(response.body).not.toContain("/preview/session-1/3000/node_modules/self-cjs/server.js");
});

test("buildPreviewResponse resolves package imports aliases inside node_modules", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/imports-lib/index.js",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/imports-lib/package.json",
          {
            path: "/workspace/node_modules/imports-lib/package.json",
            size: 71,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"imports":{"#button":{"browser":"./browser/button.js"}}}',
            ),
            textContent: '{"imports":{"#button":{"browser":"./browser/button.js"}}}',
          },
        ],
        [
          "/workspace/node_modules/imports-lib/index.js",
          {
            path: "/workspace/node_modules/imports-lib/index.js",
            size: 51,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import button from "#button";\nexport default button;',
            ),
            textContent: 'import button from "#button";\nexport default button;',
          },
        ],
        [
          "/workspace/node_modules/imports-lib/browser/button.js",
          {
            path: "/workspace/node_modules/imports-lib/browser/button.js",
            size: 27,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'button';"),
            textContent: "export default 'button';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/imports-lib/browser/button.js",
  );
});

test("buildPreviewResponse resolves package imports wildcard aliases for CommonJS", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/imports-cjs/index.cjs",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/imports-cjs/package.json",
          {
            path: "/workspace/node_modules/imports-cjs/package.json",
            size: 62,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"imports":{"#internal/*":"./browser/internal/*.js"}}',
            ),
            textContent: '{"imports":{"#internal/*":"./browser/internal/*.js"}}',
          },
        ],
        [
          "/workspace/node_modules/imports-cjs/index.cjs",
          {
            path: "/workspace/node_modules/imports-cjs/index.cjs",
            size: 80,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'const card = require("#internal/card");\nmodule.exports = { card };',
            ),
            textContent: 'const card = require("#internal/card");\nmodule.exports = { card };',
          },
        ],
        [
          "/workspace/node_modules/imports-cjs/browser/internal/card.js",
          {
            path: "/workspace/node_modules/imports-cjs/browser/internal/card.js",
            size: 25,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'card';"),
            textContent: "export default 'card';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).not.toContain('require("#internal/card")');
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/imports-cjs/browser/internal/card.js",
  );
});

test("buildPreviewResponse resolves workspace root package imports aliases", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/package.json",
          {
            path: "/workspace/package.json",
            size: 46,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"imports":{"#button":"./src/button.ts"}}'),
            textContent: '{"imports":{"#button":"./src/button.ts"}}',
          },
        ],
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 51,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import button from "#button";\nexport default button;',
            ),
            textContent: 'import button from "#button";\nexport default button;',
          },
        ],
        [
          "/workspace/src/button.ts",
          {
            path: "/workspace/src/button.ts",
            size: 27,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'button';"),
            textContent: "export default 'button';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/src/button.ts");
});

test("buildPreviewResponse resolves workspace root self imports through exports", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/package.json",
          {
            path: "/workspace/package.json",
            size: 68,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              '{"name":"workspace-app","exports":{"./button":"./src/button.ts"}}',
            ),
            textContent: '{"name":"workspace-app","exports":{"./button":"./src/button.ts"}}',
          },
        ],
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 64,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import button from "workspace-app/button";\nexport default button;',
            ),
            textContent: 'import button from "workspace-app/button";\nexport default button;',
          },
        ],
        [
          "/workspace/src/button.ts",
          {
            path: "/workspace/src/button.ts",
            size: 27,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'button';"),
            textContent: "export default 'button';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/src/button.ts");
});

test("buildPreviewResponse resolves package imports targets that point to external packages", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/imports-external/index.js",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/imports-external/package.json",
          {
            path: "/workspace/node_modules/imports-external/package.json",
            size: 28,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"imports":{"#dep":"dep-lib"}}'),
            textContent: '{"imports":{"#dep":"dep-lib"}}',
          },
        ],
        [
          "/workspace/node_modules/imports-external/index.js",
          {
            path: "/workspace/node_modules/imports-external/index.js",
            size: 45,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('import dep from "#dep";\nexport default dep;'),
            textContent: 'import dep from "#dep";\nexport default dep;',
          },
        ],
        [
          "/workspace/node_modules/imports-external/node_modules/dep-lib/package.json",
          {
            path: "/workspace/node_modules/imports-external/node_modules/dep-lib/package.json",
            size: 24,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"main":"./nested.js"}'),
            textContent: '{"main":"./nested.js"}',
          },
        ],
        [
          "/workspace/node_modules/imports-external/node_modules/dep-lib/nested.js",
          {
            path: "/workspace/node_modules/imports-external/node_modules/dep-lib/nested.js",
            size: 24,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'nested';"),
            textContent: "export default 'nested';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/imports-external/node_modules/dep-lib/nested.js",
  );
});

test("buildPreviewResponse rewrites blocked package imports aliases to fail-fast modules", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/imports-blocked/index.js",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/imports-blocked/package.json",
          {
            path: "/workspace/node_modules/imports-blocked/package.json",
            size: 31,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"imports":{"#server":null}}'),
            textContent: '{"imports":{"#server":null}}',
          },
        ],
        [
          "/workspace/node_modules/imports-blocked/index.js",
          {
            path: "/workspace/node_modules/imports-blocked/index.js",
            size: 45,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import server from "#server";\nexport default server;',
            ),
            textContent: 'import server from "#server";\nexport default server;',
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("data:text/javascript;charset=utf-8,");
  expect(response.body).toContain("NodeInNodeResolutionError");
  expect(response.body).toContain("%23server");
  expect(response.body).toContain("package.json%20imports");
});

test("buildPreviewResponse rewrites blocked package exports to fail-fast modules", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/src/main.tsx",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/src/main.tsx",
          {
            path: "/workspace/src/main.tsx",
            size: 59,
            contentType: "text/plain; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode(
              'import secret from "blocked-lib/secret";\nexport default secret;',
            ),
            textContent: 'import secret from "blocked-lib/secret";\nexport default secret;',
          },
        ],
        [
          "/workspace/node_modules/blocked-lib/package.json",
          {
            path: "/workspace/node_modules/blocked-lib/package.json",
            size: 31,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"exports":{"./secret":null}}'),
            textContent: '{"exports":{"./secret":null}}',
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("data:text/javascript;charset=utf-8,");
  expect(response.body).toContain("NodeInNodeResolutionError");
  expect(response.body).toContain(".%2Fsecret");
  expect(response.body).toContain("package.json%20exports");
});

test("buildPreviewResponse prefers nested node_modules dependencies over workspace root ones", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/parent-lib/index.js",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/parent-lib/index.js",
          {
            path: "/workspace/node_modules/parent-lib/index.js",
            size: 49,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('import dep from "nested-dep";\nexport default dep;'),
            textContent: 'import dep from "nested-dep";\nexport default dep;',
          },
        ],
        [
          "/workspace/node_modules/parent-lib/node_modules/nested-dep/package.json",
          {
            path: "/workspace/node_modules/parent-lib/node_modules/nested-dep/package.json",
            size: 24,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"main":"./nested.js"}'),
            textContent: '{"main":"./nested.js"}',
          },
        ],
        [
          "/workspace/node_modules/parent-lib/node_modules/nested-dep/nested.js",
          {
            path: "/workspace/node_modules/parent-lib/node_modules/nested-dep/nested.js",
            size: 27,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'nested';"),
            textContent: "export default 'nested';",
          },
        ],
        [
          "/workspace/node_modules/nested-dep/package.json",
          {
            path: "/workspace/node_modules/nested-dep/package.json",
            size: 22,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"main":"./root.js"}'),
            textContent: '{"main":"./root.js"}',
          },
        ],
        [
          "/workspace/node_modules/nested-dep/root.js",
          {
            path: "/workspace/node_modules/nested-dep/root.js",
            size: 25,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'root';"),
            textContent: "export default 'root';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain(
    "/preview/session-1/3000/node_modules/parent-lib/node_modules/nested-dep/nested.js",
  );
  expect(response.body).not.toContain("/preview/session-1/3000/node_modules/nested-dep/root.js");
});

test("buildPreviewResponse falls back to workspace root node_modules when nested dependency is absent", () => {
  const response = buildPreviewResponse(
    {
      ...request,
      pathname: "/preview/session-1/3000/node_modules/parent-lib/index.js",
    },
    createPreviewState(
      new Map([
        [
          "/workspace/node_modules/parent-lib/index.js",
          {
            path: "/workspace/node_modules/parent-lib/index.js",
            size: 49,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('import dep from "nested-dep";\nexport default dep;'),
            textContent: 'import dep from "nested-dep";\nexport default dep;',
          },
        ],
        [
          "/workspace/node_modules/nested-dep/package.json",
          {
            path: "/workspace/node_modules/nested-dep/package.json",
            size: 22,
            contentType: "application/json; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode('{"main":"./root.js"}'),
            textContent: '{"main":"./root.js"}',
          },
        ],
        [
          "/workspace/node_modules/nested-dep/root.js",
          {
            path: "/workspace/node_modules/nested-dep/root.js",
            size: 25,
            contentType: "text/javascript; charset=utf-8",
            isText: true,
            bytes: new TextEncoder().encode("export default 'root';"),
            textContent: "export default 'root';",
          },
        ],
      ]),
    ),
  );

  expect(response.status).toBe(200);
  expect(typeof response.body).toBe("string");
  expect(response.body).toContain("/preview/session-1/3000/node_modules/nested-dep/root.js");
});

test("buildPreviewResponse returns 404 for missing preview", () => {
  const response = buildPreviewResponse(request, null);

  expect(response.status).toBe(404);
  expect(response.body).toContain("Preview session not found");
});

test("isPreviewPath matches preview routes", () => {
  expect(isPreviewPath("/preview/session-1/3000/")).toBe(true);
  expect(isPreviewPath("/_preview/data/session-1/3000")).toBe(false);
});
