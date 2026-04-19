import ts from "typescript";

import previewEmptyModuleUrl from "../preview/shims/empty-module.js?url";
import previewJsxDevRuntimeUrl from "../preview/shims/jsx-dev-runtime.js?url";
import previewJsxRuntimeUrl from "../preview/shims/jsx-runtime.js?url";
import previewReactDomClientUrl from "../preview/shims/react-dom-client.js?url";
import previewReactUrl from "../preview/shims/react.js?url";
import previewStylesheetShimUrl from "../preview/shims/stylesheet.js?url";
import type {
  PreviewModel,
  PreviewReadyEvent,
  PreviewWorkspaceFile,
  SessionId,
  SessionSnapshot,
  VirtualHttpRequest,
  VirtualHttpResponse,
} from "./protocol";
import type { WorkspaceFileRecord } from "./analyze-archive";
import type { HostPreviewRootHint } from "./host-adapter";
import { PREVIEW_CLIENT_HEADER } from "./preview-constants";

type PreviewServerState = {
  sessionId: SessionId;
  pid: number;
  port: number;
  url: string;
  model: PreviewModel;
  rootHint?: HostPreviewRootHint;
  session: SessionSnapshot;
  files: Map<string, WorkspaceFileRecord>;
};

type PackageExportValue =
  | string
  | null
  | {
      browser?: PackageExportValue;
      import?: PackageExportValue;
      default?: PackageExportValue;
      module?: PackageExportValue;
      require?: PackageExportValue;
      [key: string]: PackageExportValue | undefined;
    };

type PackageManifest = {
  name?: string;
  browser?: string | Record<string, string | false>;
  module?: string;
  main?: string;
  exports?: PackageExportValue;
  imports?: {
    [key: string]: PackageExportValue | undefined;
  };
};

type PackageTargetResolution = string | null | undefined;

export const PREVIEW_DOCUMENT_CANDIDATES = [
  "/workspace/index.html",
  "/workspace/dist/index.html",
  "/workspace/build/index.html",
  "/workspace/public/index.html",
] as const;

export const PREVIEW_APP_ENTRY_CANDIDATES = [
  "/workspace/src/main.tsx",
  "/workspace/src/main.jsx",
  "/workspace/src/main.ts",
  "/workspace/src/main.js",
  "/workspace/src/index.tsx",
  "/workspace/src/index.jsx",
  "/workspace/src/index.ts",
  "/workspace/src/index.js",
] as const;

export function buildPreviewResponse(
  request: VirtualHttpRequest,
  preview: PreviewServerState | null,
): VirtualHttpResponse {
  if (!preview || preview.port !== request.port || preview.sessionId !== request.sessionId) {
    return jsonResponse(404, {
      error: "Preview session not found",
      sessionId: request.sessionId,
      port: request.port,
    });
  }

  const relativePath = getPreviewRelativePath(request, preview);

  if (relativePath === "/" || relativePath === "/index.html") {
    const workspaceDocument = resolvePreviewDocument(preview);

    if (workspaceDocument) {
      return buildWorkspaceFileResponse(workspaceDocument.file, preview, workspaceDocument.root);
    }

    const workspaceEntry = resolvePreviewAppEntry(preview);

    if (workspaceEntry) {
      return htmlResponse(
        200,
        renderPreviewAppShell({
          title: preview.model.title,
          entryUrl: buildPreviewUrlForWorkspaceFile(workspaceEntry.path, preview, "/workspace"),
        }),
      );
    }

    const clientScriptUrl = request.headers[PREVIEW_CLIENT_HEADER];

    if (!clientScriptUrl) {
      return htmlResponse(503, renderPreviewError("Preview client script is not configured."));
    }

    return htmlResponse(
      200,
      renderPreviewHtml({
        preview,
        clientScriptUrl,
        stateUrl: `${preview.url}__runtime.json`,
        workspaceUrl: `${preview.url}__workspace.json`,
        filesUrl: `${preview.url}__files.json`,
        stylesheetUrl: `${preview.url}assets/runtime.css`,
      }),
    );
  }

  if (relativePath === "/__runtime.json") {
    return jsonResponse(200, {
      type: "preview.ready",
      sessionId: preview.sessionId,
      pid: preview.pid,
      port: preview.port,
      url: preview.url,
      model: preview.model,
    } satisfies PreviewReadyEvent);
  }

  if (relativePath === "/__workspace.json") {
    return jsonResponse(200, preview.session);
  }

  if (relativePath === "/__files.json") {
    return jsonResponse(200, buildPreviewFileIndex(preview));
  }

  if (relativePath === "/assets/runtime.css") {
    return cssResponse(200, renderRuntimeStylesheet());
  }

  if (relativePath.startsWith("/files/")) {
    return buildPreviewFileResponse(relativePath, preview);
  }

  const workspaceAsset = buildWorkspaceAssetResponse(relativePath, preview);

  if (workspaceAsset) {
    return workspaceAsset;
  }

  return jsonResponse(404, {
    error: "Unsupported preview path",
    pathname: request.pathname,
  });
}

export function isPreviewPath(pathname: string): boolean {
  return pathname.startsWith("/preview/");
}

function jsonResponse(status: number, body: unknown): VirtualHttpResponse {
  return {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
    body: JSON.stringify(body),
  };
}

function htmlResponse(status: number, body: string): VirtualHttpResponse {
  return {
    status,
    headers: {
      "content-type": "text/html; charset=utf-8",
      "cache-control": "no-store",
    },
    body,
  };
}

function cssResponse(status: number, body: string): VirtualHttpResponse {
  return {
    status,
    headers: {
      "content-type": "text/css; charset=utf-8",
      "cache-control": "no-store",
    },
    body,
  };
}

function renderPreviewHtml(input: {
  preview: PreviewServerState;
  clientScriptUrl: string;
  stateUrl: string;
  workspaceUrl: string;
  filesUrl: string;
  stylesheetUrl: string;
}): string {
  return `<!doctype html>
  <html lang="ja">
    <head>
      <meta charset="UTF-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1.0" />
      <title>${escapeHtml(input.preview.model.title)}</title>
      <link rel="stylesheet" href="${input.stylesheetUrl}" />
      <style>
        :root {
          color-scheme: dark;
          font-family: "Iowan Old Style", "Palatino Linotype", serif;
          background:
            radial-gradient(circle at top, rgba(245, 158, 11, 0.18), transparent 35%),
            linear-gradient(160deg, #08111f 0%, #101b2f 50%, #1c2940 100%);
          color: #f5f7fb;
        }

        * {
          box-sizing: border-box;
        }

        html,
        body,
        #guest-root {
          min-height: 100%;
        }

        body {
          margin: 0;
        }
      </style>
    </head>
    <body>
      <div id="guest-root"></div>
      <script>
        window.__NODE_IN_NODE_PREVIEW__ = {
          sessionId: ${JSON.stringify(input.preview.sessionId)},
          port: ${JSON.stringify(input.preview.port)},
          stateUrl: ${JSON.stringify(input.stateUrl)},
          workspaceUrl: ${JSON.stringify(input.workspaceUrl)},
          filesUrl: ${JSON.stringify(input.filesUrl)}
        };
      </script>
      <script type="module" src="${input.clientScriptUrl}"></script>
    </body>
  </html>`;
}

function renderPreviewAppShell(input: { title: string; entryUrl: string }): string {
  return `<!doctype html>
  <html lang="ja">
    <head>
      <meta charset="UTF-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1.0" />
      <title>${escapeHtml(input.title)}</title>
      <style>
        :root {
          color-scheme: dark;
          background:
            radial-gradient(circle at top, rgba(16, 185, 129, 0.12), transparent 35%),
            linear-gradient(180deg, #071018 0%, #0b1522 100%);
        }

        * {
          box-sizing: border-box;
        }

        html,
        body {
          min-height: 100%;
        }

        body {
          margin: 0;
        }

        #root,
        #app {
          min-height: 100vh;
        }
      </style>
    </head>
    <body>
      <div id="root"></div>
      <div id="app"></div>
      <script type="module" src="${input.entryUrl}"></script>
    </body>
  </html>`;
}

function renderPreviewError(message: string): string {
  return `<!doctype html>
  <html lang="en">
    <head>
      <meta charset="UTF-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1.0" />
      <title>Preview unavailable</title>
      <style>
        body {
          margin: 0;
          min-height: 100vh;
          display: grid;
          place-items: center;
          background: #0f172a;
          color: #e2e8f0;
          font-family: system-ui, sans-serif;
        }

        article {
          width: min(560px, calc(100% - 32px));
          padding: 24px;
          border-radius: 24px;
          background: rgba(15, 23, 42, 0.78);
          border: 1px solid rgba(148, 163, 184, 0.2);
        }
      </style>
    </head>
    <body>
      <article>
        <h1>Preview unavailable</h1>
        <p>${escapeHtml(message)}</p>
      </article>
    </body>
  </html>`;
}

function renderRuntimeStylesheet(): string {
  return `
    .guest-shell {
      position: relative;
    }

    .guest-shell::after {
      content: "";
      position: absolute;
      inset: auto 0 -40px auto;
      width: 220px;
      height: 220px;
      border-radius: 999px;
      background: radial-gradient(circle, rgba(249, 115, 22, 0.28), transparent 70%);
      filter: blur(10px);
      pointer-events: none;
    }

    .guest-columns {
      display: grid;
      grid-template-columns: 1.2fr 0.8fr;
      gap: 18px;
      align-items: start;
    }

    .guest-card {
      padding: 18px;
      border-radius: 18px;
      background: rgba(6, 12, 22, 0.56);
      border: 1px solid rgba(255, 255, 255, 0.08);
    }

    .guest-card h4 {
      margin: 0 0 12px;
      font-size: 0.95rem;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: rgba(245, 247, 251, 0.7);
    }

    .guest-list {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
      gap: 10px;
    }

    .guest-list li {
      display: flex;
      justify-content: space-between;
      gap: 12px;
      font-family: "SFMono-Regular", "SF Mono", monospace;
      font-size: 0.85rem;
      color: rgba(245, 247, 251, 0.9);
    }

    .guest-list span {
      color: rgba(245, 247, 251, 0.55);
      text-transform: uppercase;
      letter-spacing: 0.08em;
      font-size: 0.7rem;
    }

    @media (max-width: 760px) {
      .guest-columns {
        grid-template-columns: 1fr;
      }
    }
  `;
}

function buildPreviewFileIndex(preview: PreviewServerState): PreviewWorkspaceFile[] {
  const documentRoot = resolvePreviewDocumentRoot(preview);

  return [...preview.files.values()]
    .filter((file) => file.isText)
    .sort((left, right) => left.path.localeCompare(right.path))
    .map((file) => ({
      path: file.path,
      size: file.size,
      contentType: file.contentType,
      isText: file.isText,
      url: `${preview.url}files${file.path.replace("/workspace", "")}`,
      previewUrl: buildPreviewUrlForWorkspaceFile(file.path, preview, documentRoot),
    }));
}

function buildPreviewFileResponse(
  relativePath: string,
  preview: PreviewServerState,
): VirtualHttpResponse {
  const requestedPath = decodeWorkspacePath(relativePath);
  const file = preview.files.get(requestedPath);

  if (!file) {
    return jsonResponse(404, {
      error: "Requested file not found",
      path: requestedPath,
    });
  }

  return buildWorkspaceFileResponse(file, preview, "/workspace");
}

function buildWorkspaceAssetResponse(
  relativePath: string,
  preview: PreviewServerState,
): VirtualHttpResponse | null {
  if (relativePath.startsWith("/__")) {
    return null;
  }

  const documentRoot = resolvePreviewDocumentRoot(preview);
  const candidates = resolveWorkspaceAssetCandidates(relativePath, documentRoot);

  for (const candidate of candidates) {
    const file = preview.files.get(candidate);

    if (!file) {
      continue;
    }

    return buildWorkspaceFileResponse(file, preview, documentRoot);
  }

  return null;
}

function buildWorkspaceFileResponse(
  file: WorkspaceFileRecord,
  preview: PreviewServerState,
  documentRoot: string,
): VirtualHttpResponse {
  if (shouldTransformWorkspaceModule(file)) {
    return {
      status: 200,
      headers: {
        "content-type": "text/javascript; charset=utf-8",
        "cache-control": "no-store",
      },
      body: transformWorkspaceModule(file.textContent ?? "", file.path, preview),
    };
  }

  if (!file.isText || file.textContent === null) {
    return binaryResponse(file.contentType, file.bytes);
  }

  return {
    status: 200,
    headers: {
      "content-type": file.contentType,
      "cache-control": "no-store",
    },
    body: rewriteWorkspaceTextContent(file, preview, documentRoot),
  };
}

function binaryResponse(contentType: string, body: Uint8Array): VirtualHttpResponse {
  return {
    status: 200,
    headers: {
      "content-type": contentType,
      "cache-control": "no-store",
    },
    body,
  };
}

function shouldTransformWorkspaceModule(file: WorkspaceFileRecord): boolean {
  if (!file.isText || file.textContent === null) {
    return false;
  }

  return (
    file.path.endsWith(".js") ||
    file.path.endsWith(".mjs") ||
    file.path.endsWith(".cjs") ||
    file.path.endsWith(".ts") ||
    file.path.endsWith(".tsx") ||
    file.path.endsWith(".jsx") ||
    file.path.endsWith(".mts") ||
    file.path.endsWith(".cts")
  );
}

function transformWorkspaceModule(
  source: string,
  filePath: string,
  preview: PreviewServerState,
): string {
  if (looksLikeCommonJsModule(filePath, source)) {
    const transformed = transformCommonJsModule(source);
    const withStylesheetImports = rewriteStylesheetImports(transformed, filePath, preview);
    return rewriteModuleSpecifiers(withStylesheetImports, filePath, preview);
  }

  const transpiled = ts.transpileModule(source, {
    fileName: filePath,
    compilerOptions: {
      allowJs: true,
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
      target: ts.ScriptTarget.ES2022,
      verbatimModuleSyntax: true,
    },
    reportDiagnostics: false,
  }).outputText;

  const withStylesheetImports = rewriteStylesheetImports(transpiled, filePath, preview);
  return rewriteModuleSpecifiers(withStylesheetImports, filePath, preview);
}

function looksLikeCommonJsModule(filePath: string, source: string): boolean {
  if (filePath.endsWith(".cjs") || filePath.endsWith(".cts")) {
    return true;
  }

  return (
    /\brequire\s*\(/.test(source) ||
    /\bmodule\.exports\b/.test(source) ||
    /\bexports\.[A-Za-z_$][\w$]*\b/.test(source)
  );
}

function transformCommonJsModule(source: string): string {
  const imports: string[] = [];
  const namedExports = new Map<string, string>();
  let nextImportId = 0;
  let nextExportId = 0;
  let needsInteropHelper = false;
  let defaultExportExpression: string | null = null;

  let rewritten = source.replace(
    /^(\s*)(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*require\((["'][^"']+["'])\);?\s*$/gm,
    (_match, indent: string, localName: string, specifier: string) => {
      const importId = `__nodeInNodeImport${nextImportId++}`;
      imports.push(`import * as ${importId} from ${specifier};`);
      needsInteropHelper = true;
      return `${indent}const ${localName} = __nodeInNodeCjsInterop(${importId});`;
    },
  );

  rewritten = rewritten.replace(
    /^(\s*)(?:const|let|var)\s+(\{[^}]+\})\s*=\s*require\((["'][^"']+["'])\);?\s*$/gm,
    (_match, indent: string, pattern: string, specifier: string) => {
      const importId = `__nodeInNodeImport${nextImportId++}`;
      imports.push(`import * as ${importId} from ${specifier};`);
      return `${indent}const ${pattern} = ${importId};`;
    },
  );

  rewritten = rewritten.replace(
    /^(\s*)require\((["'][^"']+["'])\);?\s*$/gm,
    (_match, indent: string, specifier: string) => `${indent}import ${specifier};`,
  );

  rewritten = rewritten.replace(
    /^(\s*)module\.exports\s*=\s*(.+);?\s*$/gm,
    (_match, indent: string, expression: string) => {
      defaultExportExpression = expression.trim().replace(/;$/, "");
      return `${indent}const __nodeInNodeDefaultExport = ${defaultExportExpression};`;
    },
  );

  rewritten = rewritten.replace(
    /^(\s*)(?:module\.)?exports\.([A-Za-z_$][\w$]*)\s*=\s*(.+);?\s*$/gm,
    (_match, indent: string, exportName: string, expression: string) => {
      const localName = `__nodeInNodeExport${nextExportId++}`;
      namedExports.set(exportName, localName);
      return `${indent}const ${localName} = ${expression.trim().replace(/;$/, "")};`;
    },
  );

  if (namedExports.size > 0) {
    rewritten = rewriteCommonJsExportReferences(rewritten, namedExports);
  }

  const prologue: string[] = [];

  if (imports.length > 0) {
    prologue.push(...imports);
  }

  if (needsInteropHelper) {
    prologue.push(
      "const __nodeInNodeCjsInterop = (mod) => (mod && typeof mod === 'object' && 'default' in mod ? mod.default : mod);",
    );
  }

  const epilogue: string[] = [];

  if (namedExports.size > 0) {
    epilogue.push(
      `export { ${[...namedExports.entries()]
        .map(([exportName, localName]) => `${localName} as ${exportName}`)
        .join(", ")} };`,
    );
  }

  if (defaultExportExpression !== null) {
    epilogue.push("export default __nodeInNodeDefaultExport;");
  }

  return [...prologue, rewritten, ...epilogue].filter(Boolean).join("\n");
}

function rewriteCommonJsExportReferences(
  source: string,
  namedExports: Map<string, string>,
): string {
  let rewritten = source;

  for (const [exportName, localName] of namedExports.entries()) {
    rewritten = rewritten.replace(
      new RegExp(`\\b(?:module\\.)?exports\\.${escapeRegExp(exportName)}\\b`, "g"),
      localName,
    );
  }

  return rewritten;
}

function rewriteWorkspaceTextContent(
  file: WorkspaceFileRecord,
  preview: PreviewServerState,
  documentRoot: string,
): string {
  if (file.contentType.startsWith("text/html")) {
    return rewriteHtmlDocument(file.textContent ?? "", preview.url);
  }

  if (file.contentType.startsWith("text/css")) {
    return rewriteStylesheet(file.textContent ?? "", preview.url);
  }

  if (file.contentType.startsWith("image/svg+xml")) {
    return rewriteSvgDocument(
      file.textContent ?? "",
      buildPreviewUrlForWorkspaceFile(file.path, preview, documentRoot),
    );
  }

  return file.textContent ?? "";
}

function rewriteModuleSpecifiers(
  source: string,
  importerPath: string,
  preview: PreviewServerState,
): string {
  return source
    .replace(
      /\b(from\s*["'])([^"']+)(["'])/g,
      (_match, prefix: string, specifier: string, suffix: string) => {
        return `${prefix}${resolvePreviewModuleSpecifier(specifier, importerPath, preview)}${suffix}`;
      },
    )
    .replace(
      /\b(import\s*\(\s*["'])([^"']+)(["']\s*\))/g,
      (_match, prefix: string, specifier: string, suffix: string) => {
        return `${prefix}${resolvePreviewModuleSpecifier(specifier, importerPath, preview)}${suffix}`;
      },
    )
    .replace(
      /\b(import\s*["'])([^"']+)(["'])/g,
      (_match, prefix: string, specifier: string, suffix: string) => {
        return `${prefix}${resolvePreviewModuleSpecifier(specifier, importerPath, preview)}${suffix}`;
      },
    );
}

function rewriteStylesheetImports(
  source: string,
  importerPath: string,
  preview: PreviewServerState,
): string {
  const stylesheetImports = [...source.matchAll(/^\s*import\s+["']([^"']+\.css)["'];?\s*$/gm)];

  if (stylesheetImports.length === 0) {
    return source;
  }

  const rewritten = stylesheetImports.reduce((current, match) => {
    const specifier = match[1];
    const statement = match[0];
    const resolvedSpecifier = resolvePreviewModuleSpecifier(specifier, importerPath, preview);
    return current.replace(
      statement,
      `__nodeInNodeAttachStylesheet(${JSON.stringify(resolvedSpecifier)});`,
    );
  }, source);

  return `import { attachStylesheet as __nodeInNodeAttachStylesheet } from ${JSON.stringify(previewStylesheetShimUrl)};\n${rewritten}`;
}

function resolvePreviewModuleSpecifier(
  specifier: string,
  importerPath: string,
  preview: PreviewServerState,
): string {
  const shim = resolvePreviewShimSpecifier(specifier);

  if (shim) {
    return shim;
  }

  if (isExternalModuleSpecifier(specifier)) {
    return specifier;
  }

  const resolvedPath = resolveWorkspaceModuleSpecifier(specifier, importerPath, preview);

  if (!resolvedPath) {
    return specifier;
  }

  if (isDirectPreviewModuleUrl(resolvedPath)) {
    return resolvedPath;
  }

  return buildPreviewUrlForWorkspaceFile(
    resolvedPath,
    preview,
    resolvePreviewDocumentRoot(preview),
  );
}

function resolvePreviewShimSpecifier(specifier: string): string | null {
  switch (specifier) {
    case "react":
      return previewReactUrl;
    case "react-dom/client":
      return previewReactDomClientUrl;
    case "react/jsx-runtime":
      return previewJsxRuntimeUrl;
    case "react/jsx-dev-runtime":
      return previewJsxDevRuntimeUrl;
    default:
      return null;
  }
}

function isExternalModuleSpecifier(specifier: string): boolean {
  return (
    specifier.startsWith("http://") ||
    specifier.startsWith("https://") ||
    specifier.startsWith("//") ||
    specifier.startsWith("data:") ||
    specifier.startsWith("blob:") ||
    specifier.startsWith("/assets/")
  );
}

function isDirectPreviewModuleUrl(specifier: string): boolean {
  return (
    isExternalModuleSpecifier(specifier) ||
    (specifier.startsWith("/") && !specifier.startsWith("/workspace"))
  );
}

function resolveWorkspaceModuleSpecifier(
  specifier: string,
  importerPath: string,
  preview: PreviewServerState,
): string | null {
  if (specifier.startsWith("/")) {
    return resolveWorkspacePathCandidates(`/workspace${specifier}`, preview.files);
  }

  if (specifier.startsWith(".")) {
    const browserMappedPath = resolvePackageRelativeBrowserSpecifier(
      specifier,
      importerPath,
      preview,
    );

    if (browserMappedPath) {
      return browserMappedPath;
    }

    const basePath = normalizePosixPath(`${dirname(importerPath)}/${specifier}`);
    return resolveWorkspacePathCandidates(basePath, preview.files);
  }

  const packageImportSpecifier = resolvePackageImportSpecifier(specifier, importerPath, preview);

  if (packageImportSpecifier) {
    return packageImportSpecifier;
  }

  const packageSelfSpecifier = resolvePackageSelfSpecifier(specifier, importerPath, preview);

  if (packageSelfSpecifier) {
    return packageSelfSpecifier;
  }

  return resolveNodeModuleSpecifier(specifier, importerPath, preview);
}

function resolvePackageImportSpecifier(
  specifier: string,
  importerPath: string,
  preview: PreviewServerState,
): string | null {
  if (!specifier.startsWith("#")) {
    return null;
  }

  const packageRoot = resolveNearestPackageRoot(importerPath, preview.files);

  if (!packageRoot) {
    return null;
  }

  const manifest = readPackageManifest(packageRoot, preview.files);
  const importTarget = resolvePackageImportTarget(manifest?.imports, specifier);

  if (importTarget === undefined) {
    return null;
  }

  if (importTarget === null) {
    return createBlockedModuleSpecifier(
      `Package import specifier "${specifier}" is blocked by package.json imports in ${packageRoot}.`,
    );
  }

  return resolvePackageImportTargetSpecifier(importTarget, packageRoot, preview);
}

function resolvePackageRelativeBrowserSpecifier(
  specifier: string,
  importerPath: string,
  preview: PreviewServerState,
): string | null {
  const packageRoot = resolveNearestPackageRoot(importerPath, preview.files);

  if (!packageRoot) {
    return null;
  }

  const manifest = readPackageManifest(packageRoot, preview.files);

  if (!manifest || typeof manifest.browser !== "object" || manifest.browser === null) {
    return null;
  }

  const requestedPath = normalizePosixPath(`${dirname(importerPath)}/${specifier}`);
  const browserSubpath = toPackageBrowserSubpath(requestedPath, packageRoot);

  if (!browserSubpath) {
    return null;
  }

  const mapped = resolveBrowserObjectMapping(manifest.browser, browserSubpath);

  if (mapped === null) {
    return null;
  }

  if (mapped === false) {
    return previewEmptyModuleUrl;
  }

  return resolveWorkspacePathCandidates(
    normalizePosixPath(`${packageRoot}/${mapped}`),
    preview.files,
  );
}

function resolveNodeModuleSpecifier(
  specifier: string,
  importerPath: string,
  preview: PreviewServerState,
): string | null {
  const parts = specifier.split("/");

  const packageName =
    specifier.startsWith("@") && parts.length >= 2 ? parts.slice(0, 2).join("/") : parts[0];

  if (!packageName) {
    return null;
  }

  const remainder = specifier.slice(packageName.length).replace(/^\/+/, "");

  for (const packageRoot of resolveNodeModuleSearchRoots(importerPath, packageName)) {
    const manifest = readPackageManifest(packageRoot, preview.files);
    const resolved = resolvePackageSpecifierFromRoot(
      packageRoot,
      remainder,
      preview.files,
      manifest,
    );

    if (resolved) {
      return resolved;
    }
  }

  return null;
}

function resolvePackageSelfSpecifier(
  specifier: string,
  importerPath: string,
  preview: PreviewServerState,
): string | null {
  const packageRoot = resolveNearestPackageRoot(importerPath, preview.files);

  if (!packageRoot) {
    return null;
  }

  const manifest = readPackageManifest(packageRoot, preview.files);
  const packageName = manifest?.name;

  if (!packageName || (specifier !== packageName && !specifier.startsWith(`${packageName}/`))) {
    return null;
  }

  const remainder = specifier.slice(packageName.length).replace(/^\/+/, "");

  return resolvePackageSpecifierFromRoot(packageRoot, remainder, preview.files, manifest);
}

function resolvePackageSpecifierFromRoot(
  packageRoot: string,
  remainder: string,
  files: Map<string, WorkspaceFileRecord>,
  manifest: PackageManifest | null,
): string | null {
  const subpath = remainder.length > 0 ? `./${remainder}` : ".";

  const exportedEntry = resolvePackageEntrypoint(packageRoot, subpath, files, manifest);

  if (exportedEntry) {
    return exportedEntry;
  }

  const browserMappedEntry = resolvePackageBrowserSubpath(packageRoot, subpath, files, manifest);

  if (browserMappedEntry) {
    return browserMappedEntry;
  }

  if (remainder.length > 0) {
    return resolveWorkspacePathCandidates(`${packageRoot}/${remainder}`, files);
  }

  const packageEntry = resolvePackageLegacyEntrypoint(packageRoot, files, manifest);
  if (packageEntry) {
    return packageEntry;
  }

  return resolveWorkspacePathCandidates(packageRoot, files);
}

function resolvePackageEntrypoint(
  packageRoot: string,
  subpath: string,
  files: Map<string, WorkspaceFileRecord>,
  manifest: PackageManifest | null,
): string | null {
  if (!manifest?.exports) {
    return null;
  }

  const exportTarget = resolvePackageExportTarget(manifest.exports, subpath);

  if (exportTarget === undefined) {
    return null;
  }

  if (exportTarget === null) {
    return createBlockedModuleSpecifier(
      `Package export "${subpath}" is blocked by package.json exports in ${packageRoot}.`,
    );
  }

  return resolveWorkspacePathCandidates(
    normalizePosixPath(`${packageRoot}/${exportTarget}`),
    files,
  );
}

function resolvePackageLegacyEntrypoint(
  packageRoot: string,
  files: Map<string, WorkspaceFileRecord>,
  manifest: PackageManifest | null,
): string | null {
  if (manifest) {
    const browserEntry = typeof manifest.browser === "string" ? manifest.browser : undefined;

    const candidates = [browserEntry, manifest.module, manifest.main]
      .filter((value): value is string => typeof value === "string" && value.length > 0)
      .map((value) => normalizePosixPath(`${packageRoot}/${value}`));

    for (const candidate of candidates) {
      const browserMappedCandidate = resolveLegacyBrowserEntry(
        candidate,
        packageRoot,
        files,
        manifest,
      );

      if (browserMappedCandidate && isDirectPreviewModuleUrl(browserMappedCandidate)) {
        return browserMappedCandidate;
      }

      const resolved = resolveWorkspacePathCandidates(browserMappedCandidate ?? candidate, files);
      if (resolved) {
        return resolved;
      }
    }
  }

  return resolveWorkspacePathCandidates(packageRoot, files);
}

function resolveWorkspacePathCandidates(
  basePath: string,
  files: Map<string, WorkspaceFileRecord>,
): string | null {
  const candidates = new Set<string>();
  const normalizedBase = normalizePosixPath(basePath);

  candidates.add(normalizedBase);

  for (const extension of [".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx", ".json"]) {
    candidates.add(`${normalizedBase}${extension}`);
    candidates.add(`${normalizedBase}/index${extension}`);
  }

  if (files.has(`${normalizedBase}/package.json`)) {
    const packageEntry =
      resolvePackageEntrypoint(
        normalizedBase,
        ".",
        files,
        readPackageManifest(normalizedBase, files),
      ) ??
      resolvePackageLegacyEntrypoint(
        normalizedBase,
        files,
        readPackageManifest(normalizedBase, files),
      );
    if (packageEntry) {
      candidates.add(packageEntry);
    }
  }

  for (const candidate of candidates) {
    if (files.has(candidate)) {
      return candidate;
    }
  }

  return null;
}

function resolvePackageExportTarget(
  exportsField: PackageExportValue | undefined,
  subpath: string,
): PackageTargetResolution {
  if (exportsField === undefined) {
    return undefined;
  }

  if (typeof exportsField === "string") {
    return subpath === "." ? exportsField : undefined;
  }

  if (exportsField === null) {
    return subpath === "." ? null : undefined;
  }

  if (hasConditionalKeys(exportsField)) {
    return subpath === "." ? resolveConditionalExportValue(exportsField) : undefined;
  }

  const directMatch = exportsField[subpath];
  if (directMatch !== undefined) {
    return resolveConditionalExportValue(directMatch);
  }

  const rootMatch = exportsField["."];
  if (subpath === ".") {
    if (rootMatch !== undefined) {
      return resolveConditionalExportValue(rootMatch);
    }
  }

  const wildcardMatch = resolveWildcardExportTarget(exportsField, subpath);
  if (wildcardMatch !== undefined) {
    return wildcardMatch;
  }

  return undefined;
}

function resolvePackageImportTarget(
  importsField:
    | {
        [key: string]: PackageExportValue | undefined;
      }
    | undefined,
  specifier: string,
): PackageTargetResolution {
  if (!importsField) {
    return undefined;
  }

  const directMatch = importsField[specifier];
  if (directMatch !== undefined) {
    return resolveConditionalExportValue(directMatch);
  }

  return resolveWildcardExportTarget(importsField, specifier);
}

function resolvePackageImportTargetSpecifier(
  importTarget: string,
  packageRoot: string,
  preview: PreviewServerState,
): string | null {
  if (isDirectPreviewModuleUrl(importTarget)) {
    return importTarget;
  }

  if (importTarget.startsWith("#")) {
    return resolvePackageImportSpecifier(importTarget, `${packageRoot}/package.json`, preview);
  }

  if (importTarget.startsWith("/")) {
    return resolveWorkspacePathCandidates(`/workspace${importTarget}`, preview.files);
  }

  if (importTarget.startsWith(".")) {
    return resolveWorkspacePathCandidates(
      normalizePosixPath(`${packageRoot}/${importTarget}`),
      preview.files,
    );
  }

  return resolveWorkspaceModuleSpecifier(importTarget, `${packageRoot}/package.json`, preview);
}

function resolveWildcardExportTarget(
  exportsField: {
    [key: string]: PackageExportValue | undefined;
  },
  subpath: string,
): PackageTargetResolution {
  for (const [key, value] of Object.entries(exportsField)) {
    if (!key.includes("*")) {
      continue;
    }

    const [prefix, suffix] = key.split("*");

    if (!subpath.startsWith(prefix) || !subpath.endsWith(suffix ?? "")) {
      continue;
    }

    const matched = subpath.slice(prefix.length, subpath.length - (suffix?.length ?? 0));
    const target = resolveConditionalExportValue(value);

    if (target === undefined) {
      continue;
    }

    if (target === null) {
      return null;
    }

    return target.replaceAll("*", matched);
  }

  return undefined;
}

function resolveConditionalExportValue(
  value: PackageExportValue | undefined,
): PackageTargetResolution {
  if (value === undefined) {
    return undefined;
  }

  if (typeof value === "string") {
    return value;
  }

  if (value === null) {
    return null;
  }

  for (const condition of ["browser", "import", "module", "default", "require"]) {
    const nested = value[condition];
    const resolved = resolveConditionalExportValue(nested);
    if (resolved !== undefined) {
      return resolved;
    }
  }

  return undefined;
}

function hasConditionalKeys(
  value: PackageExportValue,
): value is Exclude<PackageExportValue, string> {
  return (
    typeof value === "object" &&
    value !== null &&
    (Object.hasOwn(value, "browser") ||
      Object.hasOwn(value, "import") ||
      Object.hasOwn(value, "module") ||
      Object.hasOwn(value, "default") ||
      Object.hasOwn(value, "require"))
  );
}

function resolvePackageBrowserSubpath(
  packageRoot: string,
  subpath: string,
  files: Map<string, WorkspaceFileRecord>,
  manifest: PackageManifest | null,
): string | null {
  if (!manifest || typeof manifest.browser !== "object" || manifest.browser === null) {
    return null;
  }

  const mapped = resolveBrowserObjectMapping(manifest.browser, subpath);

  if (mapped === null) {
    return null;
  }

  if (mapped === false) {
    return previewEmptyModuleUrl;
  }

  return resolveWorkspacePathCandidates(normalizePosixPath(`${packageRoot}/${mapped}`), files);
}

function resolveLegacyBrowserEntry(
  candidatePath: string,
  packageRoot: string,
  files: Map<string, WorkspaceFileRecord>,
  manifest: PackageManifest | null,
): string | null {
  if (!manifest || typeof manifest.browser !== "object" || manifest.browser === null) {
    return null;
  }

  const relativePath = `.${candidatePath.slice(packageRoot.length)}`;
  const mapped = resolveBrowserObjectMapping(manifest.browser, relativePath);

  if (mapped === null) {
    return null;
  }

  if (mapped === false) {
    return previewEmptyModuleUrl;
  }

  return resolveWorkspacePathCandidates(normalizePosixPath(`${packageRoot}/${mapped}`), files);
}

function resolveBrowserObjectMapping(
  browserField: Record<string, string | false>,
  subpath: string,
): string | false | null {
  for (const candidate of buildBrowserSubpathCandidates(subpath)) {
    const mapped = browserField[candidate];

    if (typeof mapped === "string" && mapped.length > 0) {
      return mapped;
    }

    if (mapped === false) {
      return false;
    }
  }

  return null;
}

function buildBrowserSubpathCandidates(subpath: string): string[] {
  const normalized = normalizeBrowserSubpath(subpath);
  const candidates = new Set<string>();

  candidates.add(normalized);

  for (const extension of [".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx", ".json"]) {
    candidates.add(`${normalized}${extension}`);
    candidates.add(`${normalized}/index${extension}`);
  }

  return [...candidates];
}

function normalizeBrowserSubpath(subpath: string): string {
  if (subpath === ".") {
    return "./index";
  }

  return subpath.startsWith("./") ? subpath : `./${subpath.replace(/^\/+/, "")}`;
}

function resolveNearestPackageRoot(
  importerPath: string,
  files: Map<string, WorkspaceFileRecord>,
): string | null {
  let current = dirname(importerPath);

  while (current.startsWith("/workspace")) {
    if (files.has(`${current}/package.json`)) {
      return current;
    }

    if (current === "/workspace") {
      break;
    }

    current = dirname(current);
  }

  return null;
}

function resolveNodeModuleSearchRoots(importerPath: string, packageName: string): string[] {
  const searchRoots = new Set<string>();
  let current = dirname(importerPath);

  while (current.startsWith("/workspace")) {
    if (current.endsWith("/node_modules")) {
      searchRoots.add(normalizePosixPath(`${current}/${packageName}`));
    } else {
      searchRoots.add(normalizePosixPath(`${current}/node_modules/${packageName}`));
    }

    if (current === "/workspace") {
      break;
    }

    current = dirname(current);
  }

  searchRoots.add(normalizePosixPath(`/workspace/node_modules/${packageName}`));

  return [...searchRoots];
}

function toPackageBrowserSubpath(requestedPath: string, packageRoot: string): string | null {
  if (requestedPath === packageRoot) {
    return ".";
  }

  if (!requestedPath.startsWith(`${packageRoot}/`)) {
    return null;
  }

  return `.${requestedPath.slice(packageRoot.length)}`;
}

function readPackageManifest(
  packageRoot: string,
  files: Map<string, WorkspaceFileRecord>,
): PackageManifest | null {
  const packageJson = files.get(`${packageRoot}/package.json`);

  if (!packageJson?.textContent) {
    return null;
  }

  try {
    return JSON.parse(packageJson.textContent) as PackageManifest;
  } catch {
    return null;
  }
}

function createBlockedModuleSpecifier(message: string): string {
  const source = [
    `const error = new Error(${JSON.stringify(message)});`,
    'error.name = "NodeInNodeResolutionError";',
    "throw error;",
  ].join("\n");

  return `data:text/javascript;charset=utf-8,${encodeURIComponent(source)}`;
}

function normalizePosixPath(path: string): string {
  const isAbsolute = path.startsWith("/");
  const segments = path.split("/").filter(Boolean);
  const normalized: string[] = [];

  for (const segment of segments) {
    if (segment === ".") {
      continue;
    }

    if (segment === "..") {
      normalized.pop();
      continue;
    }

    normalized.push(segment);
  }

  return `${isAbsolute ? "/" : ""}${normalized.join("/")}`;
}

function resolvePreviewDocument(preview: PreviewServerState): {
  file: WorkspaceFileRecord;
  root: string;
} | null {
  if (preview.rootHint?.kind === "workspace-document") {
    const file = preview.files.get(preview.rootHint.path);

    if (file && file.isText && file.contentType.startsWith("text/html")) {
      return {
        file,
        root: preview.rootHint.root,
      };
    }
  }

  for (const candidate of PREVIEW_DOCUMENT_CANDIDATES) {
    const file = preview.files.get(candidate);

    if (file && file.isText && file.contentType.startsWith("text/html")) {
      return {
        file,
        root: dirname(candidate),
      };
    }
  }

  return null;
}

function resolvePreviewAppEntry(preview: PreviewServerState): WorkspaceFileRecord | null {
  if (preview.rootHint?.kind === "source-entry") {
    const file = preview.files.get(preview.rootHint.path);

    if (file && shouldTransformWorkspaceModule(file)) {
      return file;
    }
  }

  for (const candidate of PREVIEW_APP_ENTRY_CANDIDATES) {
    const file = preview.files.get(candidate);

    if (file && shouldTransformWorkspaceModule(file)) {
      return file;
    }
  }

  return null;
}

function resolvePreviewDocumentRoot(preview: PreviewServerState): string {
  return resolvePreviewDocument(preview)?.root ?? "/workspace";
}

function resolveWorkspaceAssetCandidates(relativePath: string, documentRoot: string): string[] {
  const normalized = normalizeWorkspaceAssetPath(relativePath);
  const candidates = new Set<string>();

  candidates.add(`${documentRoot}${normalized}`);
  candidates.add(`/workspace${normalized}`);

  if (normalized.endsWith("/")) {
    candidates.add(`${documentRoot}${normalized}index.html`);
    candidates.add(`/workspace${normalized}index.html`);
  }

  return [...candidates];
}

function normalizeWorkspaceAssetPath(relativePath: string): string {
  const normalized = relativePath.startsWith("/") ? relativePath : `/${relativePath}`;
  return normalized.replace(/\/+/g, "/");
}

function buildPreviewUrlForWorkspaceFile(
  workspacePath: string,
  preview: PreviewServerState,
  documentRoot: string,
): string {
  const effectiveRoot = workspacePath.startsWith(`${documentRoot}/`) ? documentRoot : "/workspace";
  const relative = workspacePath.slice(effectiveRoot.length).replace(/^\/+/, "");
  return relative ? `${preview.url}${relative}` : preview.url;
}

function getPreviewRelativePath(request: VirtualHttpRequest, preview: PreviewServerState): string {
  const basePath = `/preview/${preview.sessionId}/${preview.port}`;
  const suffix = request.pathname.startsWith(basePath)
    ? request.pathname.slice(basePath.length)
    : "";

  return suffix || "/";
}

function decodeWorkspacePath(relativePath: string): string {
  const encodedSuffix = relativePath.replace(/^\/files/, "");
  const suffix = encodedSuffix
    .split("/")
    .map((segment) => decodeURIComponent(segment))
    .join("/");

  return `/workspace${suffix}`;
}

function rewriteHtmlDocument(source: string, previewUrl: string): string {
  return source.replace(
    /\b(src|href|action|poster)=("|')\/(?!\/)/g,
    (_match, attribute: string, quote: string) => `${attribute}=${quote}${previewUrl}`,
  );
}

function rewriteStylesheet(source: string, previewUrl: string): string {
  return source.replace(/url\((["']?)\/(?!\/)/g, (_match, quote: string) => {
    return `url(${quote}${previewUrl}`;
  });
}

function rewriteSvgDocument(source: string, previewUrl: string): string {
  return source.replace(
    /\b(href|xlink:href)=("|')\/(?!\/)/g,
    (_match, attribute: string, quote: string) => {
      return `${attribute}=${quote}${previewUrl}`;
    },
  );
}

function escapeRegExp(input: string): string {
  return input.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function dirname(path: string): string {
  const normalized = path.replace(/\/+$/, "");
  const index = normalized.lastIndexOf("/");

  if (index <= 0) {
    return "/workspace";
  }

  return normalized.slice(0, index);
}

function escapeHtml(input: string): string {
  return input
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
