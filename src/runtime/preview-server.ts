import ts from "typescript";

import previewEmptyModuleUrl from "../preview/shims/empty-module.js?url";
import previewJsxDevRuntimeUrl from "../preview/shims/jsx-dev-runtime.js?url";
import previewJsxRuntimeUrl from "../preview/shims/jsx-runtime.js?url";
import previewReactDomClientUrl from "../preview/shims/react-dom-client.js?url";
import previewReactUrl from "../preview/shims/react.js?url";
import previewStylesheetShimUrl from "../preview/shims/stylesheet.js?url";
import type {
  PreviewHostFileSummary,
  PreviewHostSummary,
  PreviewModel,
  PreviewRunPlan,
  SessionId,
  SessionSnapshot,
  VirtualHttpRequest,
  VirtualHttpResponse,
} from "./protocol";
import type { WorkspaceFileRecord } from "./analyze-archive";
import type {
  HostPreviewRequestHint,
  HostPreviewResponseDescriptor,
  HostRuntimeModuleFormat,
  HostRuntimePreviewModulePlan,
  HostRuntimePreviewRenderPlan,
  HostRuntimePreviewTransformKind,
} from "./host-adapter";

type PreviewServerState = {
  sessionId: SessionId;
  pid: number;
  port: number;
  url: string;
  model: PreviewModel;
  rootRequestHint?: HostPreviewRequestHint;
  rootResponseDescriptor?: HostPreviewResponseDescriptor;
  requestHint?: HostPreviewRequestHint;
  responseDescriptor?: HostPreviewResponseDescriptor;
  transformKind?: HostRuntimePreviewTransformKind;
  renderPlan?: HostRuntimePreviewRenderPlan;
  modulePlan?: HostRuntimePreviewModulePlan;
  host: PreviewHostSummary;
  run: PreviewRunPlan;
  hostFiles: PreviewHostFileSummary;
  session: SessionSnapshot;
  files: Map<string, WorkspaceFileRecord>;
};

const PREVIEW_AUXILIARY_ROUTE_KINDS = new Map<string, HostPreviewRequestHint["kind"]>([
  ["/__runtime.json", "not-found"],
  ["/__bootstrap.json", "not-found"],
  ["/__workspace.json", "not-found"],
  ["/__files.json", "not-found"],
  ["/__diagnostics.json", "not-found"],
  ["/assets/runtime.css", "runtime-stylesheet"],
]);

type PreviewResponseMetadata = Pick<
  HostPreviewResponseDescriptor,
  "statusCode" | "contentType" | "allowMethods" | "omitBody" | "workspacePath"
>;

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
  if (preview.renderPlan) {
    return buildPreviewResponseFromRenderPlan(request, preview, preview.renderPlan);
  }
  if (preview.responseDescriptor) {
    return buildPreviewResponseFromDescriptor(request, preview, preview.responseDescriptor);
  }
  if ((relativePath === "/" || relativePath === "/index.html") && preview.rootResponseDescriptor) {
    return buildPreviewResponseFromDescriptor(request, preview, preview.rootResponseDescriptor);
  }
  if (preview.requestHint) {
    return buildPreviewResponseFromHint(request, preview, preview.requestHint);
  }
  if (hasBackendOwnedPreviewState(preview)) {
    return unsupportedPreviewPathResponse(request.pathname);
  }

  return buildPreviewResponseFromFallback(request, preview, relativePath);
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

function inferPreviewRequestKind(relativePath: string): HostPreviewRequestHint["kind"] {
  if (relativePath === "/" || relativePath === "/index.html") {
    return "fallback-root";
  }

  const auxiliaryRouteKind = PREVIEW_AUXILIARY_ROUTE_KINDS.get(relativePath);
  if (auxiliaryRouteKind) {
    return auxiliaryRouteKind;
  }

  if (relativePath.startsWith("/files/")) {
    return "workspace-file";
  }

  return "workspace-asset";
}

function buildPreviewResponseFromHint(
  request: VirtualHttpRequest,
  preview: PreviewServerState,
  requestHint: HostPreviewRequestHint,
): VirtualHttpResponse {
  switch (requestHint.kind) {
    case "root-document":
      return unsupportedPreviewPathResponse(request.pathname);
    case "workspace-file":
    case "workspace-asset": {
      const response = buildHintedWorkspaceModuleResponse(
        preview,
        requestHint.workspacePath,
        requestHint.documentRoot,
      );
      if (!response && preview.responseDescriptor?.omitBody) {
        return emptyPreviewResponse();
      }
      return response ?? unsupportedPreviewPathResponse(request.pathname);
    }
    case "root-entry":
    case "fallback-root":
    case "bootstrap-state":
    case "runtime-state":
    case "workspace-state":
    case "file-index":
    case "diagnostics-state":
    case "runtime-stylesheet":
      return unsupportedPreviewPathResponse(request.pathname);
    case "not-found":
      return unsupportedPreviewPathResponse(request.pathname);
  }
}

function buildPreviewResponseFromDescriptor(
  request: VirtualHttpRequest,
  preview: PreviewServerState,
  descriptor: HostPreviewResponseDescriptor,
): VirtualHttpResponse {
  const response = (() => {
    switch (descriptor.kind) {
      case "workspace-document":
        return unsupportedPreviewPathResponse(request.pathname);
      case "workspace-file":
      case "workspace-asset": {
        const hintedResponse = buildHintedWorkspaceModuleResponse(
          preview,
          descriptor.workspacePath,
          descriptor.documentRoot,
        );
        if (!hintedResponse && descriptor.omitBody) {
          return emptyPreviewResponse();
        }
        return hintedResponse ?? unsupportedPreviewPathResponse(request.pathname);
      }
      case "app-shell":
        return unsupportedPreviewPathResponse(request.pathname);
      case "host-managed-fallback":
      case "bootstrap-state":
      case "runtime-state":
      case "workspace-state":
      case "file-index":
      case "diagnostics-state":
      case "runtime-stylesheet":
        return unsupportedPreviewPathResponse(request.pathname);
      case "method-not-allowed":
        return jsonResponse(405, {
          error: "Method not allowed",
          pathname: request.pathname,
          method: request.method,
          allowMethods: descriptor.allowMethods,
        });
      case "not-found":
        return unsupportedPreviewPathResponse(request.pathname);
    }
  })();

  return response.status === 404
    ? response
    : applyDescriptorMetadata(response, descriptor, preview);
}

function buildPreviewResponseFromRenderPlan(
  request: VirtualHttpRequest,
  preview: PreviewServerState,
  renderPlan: HostRuntimePreviewRenderPlan,
): VirtualHttpResponse {
  switch (renderPlan.kind) {
    case "workspace-file": {
      if (!renderPlan.workspacePath || !renderPlan.documentRoot) {
        return unsupportedPreviewPathResponse(request.pathname);
      }
      const hintedResponse = buildHintedWorkspaceModuleResponse(
        preview,
        renderPlan.workspacePath,
        renderPlan.documentRoot,
      );
      if (!hintedResponse && preview.responseDescriptor?.omitBody) {
        return applyDescriptorMetadata(emptyPreviewResponse(), preview.responseDescriptor, preview);
      }
      const response = hintedResponse ?? unsupportedPreviewPathResponse(request.pathname);
      return preview.responseDescriptor && response.status !== 404
        ? applyDescriptorMetadata(response, preview.responseDescriptor, preview)
        : response;
    }
    case "app-shell":
      return unsupportedPreviewPathResponse(request.pathname);
    case "host-managed-fallback":
      return unsupportedPreviewPathResponse(request.pathname);
  }
}

function buildPreviewResponseFromFallback(
  request: VirtualHttpRequest,
  preview: PreviewServerState,
  relativePath: string,
): VirtualHttpResponse {
  switch (inferPreviewRequestKind(relativePath)) {
    case "workspace-file":
      return buildPreviewModuleFileResponse(relativePath, preview);
    case "workspace-asset": {
      const response = buildPreviewModuleAssetResponse(relativePath, preview);
      return response ?? unsupportedPreviewPathResponse(request.pathname);
    }
    case "fallback-root":
    case "bootstrap-state":
    case "runtime-state":
    case "workspace-state":
    case "file-index":
    case "diagnostics-state":
    case "runtime-stylesheet":
    case "root-document":
    case "root-entry":
      return unsupportedPreviewPathResponse(request.pathname);
    case "not-found":
      return unsupportedPreviewPathResponse(request.pathname);
  }
}

function buildHintedWorkspaceModuleResponse(
  preview: PreviewServerState,
  workspacePath: string,
  documentRoot: string,
): VirtualHttpResponse | null {
  const file = preview.files.get(workspacePath);
  if (!file) {
    return null;
  }

  return buildWorkspaceModuleResponse(file, preview, documentRoot);
}

function applyDescriptorMetadata(
  response: VirtualHttpResponse,
  descriptor: PreviewResponseMetadata,
  preview: PreviewServerState,
): VirtualHttpResponse {
  const headers = { ...response.headers };
  const contentType =
    preview.transformKind === "module" &&
    descriptor.workspacePath !== null &&
    descriptor.workspacePath ===
      (preview.responseDescriptor?.workspacePath ?? preview.requestHint?.workspacePath ?? null)
      ? "text/javascript; charset=utf-8"
      : descriptor.contentType;
  if (contentType) {
    headers["content-type"] = contentType;
  }
  if (descriptor.allowMethods.length > 0) {
    headers.allow = descriptor.allowMethods.join(", ");
  }

  return {
    ...response,
    status: descriptor.statusCode,
    headers,
    body: descriptor.omitBody ? "" : response.body,
  };
}

function unsupportedPreviewPathResponse(pathname: string): VirtualHttpResponse {
  return jsonResponse(404, {
    error: "Unsupported preview path",
    pathname,
  });
}

function emptyPreviewResponse(): VirtualHttpResponse {
  return {
    status: 200,
    headers: {
      "cache-control": "no-store",
    },
    body: "",
  };
}

function hasBackendOwnedPreviewState(preview: PreviewServerState): boolean {
  return Boolean(
    preview.rootRequestHint ??
    preview.rootResponseDescriptor ??
    preview.requestHint ??
    preview.responseDescriptor ??
    preview.renderPlan,
  );
}

function buildPreviewModuleFileResponse(
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

  return buildWorkspaceModuleResponse(file, preview, resolvePreviewDocumentRoot(preview));
}

function buildPreviewModuleAssetResponse(
  relativePath: string,
  preview: PreviewServerState,
): VirtualHttpResponse | null {
  if (relativePath.startsWith("/__")) {
    return null;
  }

  const documentRoot =
    preview.requestHint?.kind === "workspace-asset"
      ? preview.requestHint.documentRoot
      : resolvePreviewDocumentRoot(preview);

  if (preview.requestHint?.kind === "workspace-asset") {
    const hintedFile = preview.files.get(preview.requestHint.workspacePath);

    if (hintedFile) {
      return buildWorkspaceModuleResponse(hintedFile, preview, documentRoot);
    }
  }

  const candidates = resolveWorkspaceAssetCandidates(relativePath, documentRoot);

  for (const candidate of candidates) {
    const file = preview.files.get(candidate);

    if (!file) {
      continue;
    }

    return buildWorkspaceModuleResponse(file, preview, documentRoot);
  }

  return null;
}

function buildWorkspaceModuleResponse(
  file: WorkspaceFileRecord,
  preview: PreviewServerState,
  documentRoot: string,
): VirtualHttpResponse {
  const transformKind = resolvePreviewTransformKind(file, preview);

  if (transformKind !== "module" || file.textContent === null) {
    return unsupportedPreviewPathResponse(
      buildPreviewUrlForWorkspaceFile(file.path, preview, documentRoot),
    );
  }

  return {
    status: 200,
    headers: {
      "content-type": "text/javascript; charset=utf-8",
      "cache-control": "no-store",
    },
    body: transformWorkspaceModule(file.textContent, file.path, preview),
  };
}

function transformWorkspaceModule(
  source: string,
  filePath: string,
  preview: PreviewServerState,
): string {
  const moduleFormat = resolvePreviewModuleFormat(filePath, source, preview.modulePlan);

  if (moduleFormat === "json") {
    return `export default ${source.trim()};\n`;
  }

  if (moduleFormat === "commonjs") {
    const transformed = transformCommonJsModule(source);
    const withStylesheetImports = rewriteStylesheetImports(transformed, filePath, preview);
    const withStaticAssetImports = rewriteStaticAssetImports(
      withStylesheetImports,
      filePath,
      preview,
    );
    return rewriteModuleSpecifiers(withStaticAssetImports, filePath, preview);
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
  const withStaticAssetImports = rewriteStaticAssetImports(
    withStylesheetImports,
    filePath,
    preview,
  );
  return rewriteModuleSpecifiers(withStaticAssetImports, filePath, preview);
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

function resolvePreviewTransformKind(
  file: WorkspaceFileRecord,
  preview: PreviewServerState,
): HostRuntimePreviewTransformKind {
  const plannedTransformKind = preview.transformKind;
  const plannedWorkspacePath =
    preview.responseDescriptor?.workspacePath ?? preview.requestHint?.workspacePath ?? null;

  if (plannedTransformKind && plannedWorkspacePath === file.path) {
    return plannedTransformKind;
  }

  if (preview.modulePlan && preview.modulePlan.importerPath === file.path) {
    return "module";
  }

  if (!file.isText || file.textContent === null) {
    return "binary";
  }

  if (
    file.path.endsWith(".js") ||
    file.path.endsWith(".mjs") ||
    file.path.endsWith(".cjs") ||
    file.path.endsWith(".ts") ||
    file.path.endsWith(".tsx") ||
    file.path.endsWith(".jsx") ||
    file.path.endsWith(".mts") ||
    file.path.endsWith(".cts") ||
    file.path.endsWith(".json")
  ) {
    return "module";
  }

  if (file.contentType.startsWith("text/html")) {
    return "html-document";
  }
  if (file.contentType.startsWith("text/css")) {
    return "stylesheet";
  }
  if (file.contentType.startsWith("image/svg+xml")) {
    return "svg-document";
  }

  return "plain-text";
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

function rewriteStaticAssetImports(
  source: string,
  importerPath: string,
  preview: PreviewServerState,
): string {
  return source.replace(
    /^\s*import\s+([A-Za-z_$][\w$]*)\s+from\s+["']([^"']+)["'];?\s*$/gm,
    (statement: string, localName: string, specifier: string) => {
      const assetSpecifier = resolvePreviewAssetSpecifier(specifier, importerPath, preview);
      if (!assetSpecifier) {
        return statement;
      }

      return `const ${localName} = ${JSON.stringify(assetSpecifier)};`;
    },
  );
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

  const plannedSpecifier = resolvePreviewModuleSpecifierFromPlan(specifier, importerPath, preview);

  if (plannedSpecifier) {
    return plannedSpecifier;
  }

  if (
    (preview.modulePlan && preview.modulePlan.importerPath === importerPath) ||
    hasBackendOwnedPreviewState(preview)
  ) {
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

function resolvePreviewAssetSpecifier(
  specifier: string,
  importerPath: string,
  preview: PreviewServerState,
): string | null {
  if (isExternalModuleSpecifier(specifier)) {
    return specifier;
  }

  const resolvedPath = resolveWorkspaceModuleSpecifier(specifier, importerPath, preview);
  if (!resolvedPath) {
    return null;
  }

  if (isDirectPreviewModuleUrl(resolvedPath)) {
    return resolvedPath;
  }

  const file = preview.files.get(resolvedPath);
  if (!file) {
    return null;
  }

  if (resolvePreviewTransformKind(file, preview) === "module") {
    return null;
  }

  return buildPreviewUrlForWorkspaceFile(
    resolvedPath,
    preview,
    resolvePreviewDocumentRoot(preview),
  );
}

function resolvePreviewModuleSpecifierFromPlan(
  specifier: string,
  importerPath: string,
  preview: PreviewServerState,
): string | null {
  const modulePlan = preview.modulePlan;
  if (!modulePlan || modulePlan.importerPath !== importerPath) {
    return null;
  }

  const importPlan = modulePlan.importPlans.find((plan) => plan.requestSpecifier === specifier);
  if (!importPlan) {
    return null;
  }

  const resolvedSpecifier = importPlan.previewSpecifier;
  if (isDirectPreviewModuleUrl(resolvedSpecifier)) {
    return resolvedSpecifier;
  }

  return resolvedSpecifier;
}

function resolvePreviewModuleFormat(
  filePath: string,
  source: string,
  modulePlan?: HostRuntimePreviewModulePlan,
): HostRuntimeModuleFormat {
  if (modulePlan && modulePlan.importerPath === filePath) {
    return modulePlan.format;
  }

  return looksLikeCommonJsModule(filePath, source) ? "commonjs" : "module";
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

function resolvePreviewDocumentRoot(preview: PreviewServerState): string {
  return (
    preview.renderPlan?.documentRoot ??
    preview.responseDescriptor?.documentRoot ??
    preview.requestHint?.documentRoot ??
    preview.rootResponseDescriptor?.documentRoot ??
    preview.rootRequestHint?.documentRoot ??
    "/workspace"
  );
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
