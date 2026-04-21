import { strFromU8, unzipSync } from "fflate";

import type { ArchiveEntry, ArchiveSummary, PackageJsonSummary, SessionSnapshot } from "./protocol";

type ParsedArchive = Pick<
  SessionSnapshot,
  "archive" | "packageJson" | "workspaceRoot" | "capabilities"
>;

type CommonPrefixResult = {
  prefix: string | null;
  segments: string[];
};

export type WorkspaceFileRecord = {
  path: string;
  size: number;
  contentType: string;
  isText: boolean;
  bytes: Uint8Array;
  textContent: string | null;
};

export type MountedArchive = {
  snapshot: SessionSnapshot;
  files: Map<string, WorkspaceFileRecord>;
};

export function analyzeArchive(
  fileName: string,
  zip: ArrayBuffer,
  sessionId: string,
): SessionSnapshot {
  return mountArchive(fileName, zip, sessionId).snapshot;
}

export function mountArchive(
  fileName: string,
  zip: ArrayBuffer,
  sessionId: string,
): MountedArchive {
  const parsed = parseArchiveWithFiles(fileName, new Uint8Array(zip));

  return {
    snapshot: {
      sessionId,
      state: "mounted",
      revision: 0,
      workspaceRoot: "/workspace",
      archive: parsed.archive,
      packageJson: parsed.packageJson,
      capabilities: parsed.capabilities,
    },
    files: parsed.files,
  };
}

export function parseArchive(fileName: string, zipBytes: Uint8Array): ParsedArchive {
  const parsed = parseArchiveWithFiles(fileName, zipBytes);

  return {
    workspaceRoot: parsed.workspaceRoot,
    archive: parsed.archive,
    packageJson: parsed.packageJson,
    capabilities: parsed.capabilities,
  };
}

function parseArchiveWithFiles(
  fileName: string,
  zipBytes: Uint8Array,
): ParsedArchive & {
  files: Map<string, WorkspaceFileRecord>;
} {
  const unzipped = unzipSync(zipBytes);
  const entryNames = Object.keys(unzipped).filter(isWorkspaceArchiveEntry).sort();
  const commonPrefix = detectCommonPrefix(entryNames);
  const files = new Map<string, WorkspaceFileRecord>();

  const entries = entryNames.map((rawPath) => {
    const bytes = unzipped[rawPath];
    const normalizedPath = normalizeArchivePath(rawPath, commonPrefix);

    if (guessEntryKind(rawPath, bytes.byteLength) === "file") {
      const contentType = guessContentType(normalizedPath);
      const isText = isTextContentType(contentType);
      files.set(normalizedPath, {
        path: normalizedPath,
        size: bytes.byteLength,
        contentType,
        isText,
        bytes,
        textContent: isText ? decodeText(bytes) : null,
      });
    }

    return {
      path: normalizedPath,
      size: bytes.byteLength,
      kind: guessEntryKind(rawPath, bytes.byteLength),
    } satisfies ArchiveEntry;
  });

  const archive: ArchiveSummary = {
    fileName,
    fileCount: entries.filter((entry) => entry.kind === "file").length,
    directoryCount: entries.filter((entry) => entry.kind === "dir").length,
    entries,
    rootPrefix: commonPrefix.prefix,
  };

  const packageJsonEntry = entries.find(
    (entry) => entry.kind === "file" && entry.path === "/workspace/package.json",
  );

  const packageJson = packageJsonEntry
    ? readPackageJson(
        unzipped[restoreOriginalPath(packageJsonEntry.path, commonPrefix, entryNames)],
      )
    : null;

  const dependencies = [
    ...(packageJson?.dependencies ?? []),
    ...(packageJson?.devDependencies ?? []),
  ];

  return {
    workspaceRoot: "/workspace",
    archive,
    packageJson,
    files,
    capabilities: {
      detectedReact: dependencies.includes("react") || dependencies.includes("react-dom"),
      detectedVite: dependencies.includes("vite"),
    },
  };
}

export function normalizeArchivePath(rawPath: string, commonPrefix: CommonPrefixResult): string {
  const cleaned = rawPath.replace(/^\/+/, "").replace(/\/+/g, "/");

  if (!cleaned) {
    return "/workspace";
  }

  const segments = cleaned.split("/").filter(Boolean);
  const strippedSegments =
    commonPrefix.segments.length > 0 &&
    commonPrefix.segments.every((segment, index) => segments[index] === segment)
      ? segments.slice(commonPrefix.segments.length)
      : segments;

  if (strippedSegments.length === 0) {
    return "/workspace";
  }

  return `/workspace/${strippedSegments.join("/")}`;
}

export function detectCommonPrefix(entryNames: string[]): CommonPrefixResult {
  if (entryNames.length === 0) {
    return { prefix: null, segments: [] };
  }

  const splitEntries = entryNames.map((entry) => ({
    segments: entry.replace(/^\/+/, "").split("/").filter(Boolean),
    isDirectory: entry.endsWith("/"),
  }));

  const firstSegments = splitEntries[0]?.segments ?? [];
  const shared: string[] = [];

  for (const [index, candidate] of firstSegments.entries()) {
    const isShared = splitEntries.every(
      (entry) =>
        entry.segments[index] === candidate &&
        (entry.segments.length > index + 1 || entry.isDirectory),
    );

    if (!isShared) {
      break;
    }

    shared.push(candidate);
  }

  return {
    prefix: shared.length > 0 ? shared.join("/") : null,
    segments: shared,
  };
}

function isWorkspaceArchiveEntry(rawPath: string): boolean {
  const cleaned = rawPath.replace(/^\/+/, "").replace(/\/+/g, "/");
  if (!cleaned) {
    return false;
  }

  const segments = cleaned.split("/").filter(Boolean);
  if (segments[0] === "__MACOSX") {
    return false;
  }

  return !segments.some((segment) => segment.startsWith("._"));
}

function readPackageJson(bytes: Uint8Array): PackageJsonSummary | null {
  try {
    const parsed = JSON.parse(strFromU8(bytes)) as {
      name?: string;
      scripts?: Record<string, string>;
      dependencies?: Record<string, string>;
      devDependencies?: Record<string, string>;
    };

    return {
      name: parsed.name,
      scripts: parsed.scripts ?? {},
      dependencies: Object.keys(parsed.dependencies ?? {}),
      devDependencies: Object.keys(parsed.devDependencies ?? {}),
    };
  } catch {
    return null;
  }
}

function guessEntryKind(rawPath: string, size: number): ArchiveEntry["kind"] {
  if (rawPath.endsWith("/") || size === 0) {
    return "dir";
  }

  return "file";
}

function restoreOriginalPath(
  normalizedPath: string,
  commonPrefix: CommonPrefixResult,
  entryNames: string[],
): string {
  const suffix = normalizedPath.replace(/^\/workspace\/?/, "");

  if (commonPrefix.prefix) {
    const prefixed = `${commonPrefix.prefix}/${suffix}`.replace(/\/+/g, "/");
    if (entryNames.includes(prefixed)) {
      return prefixed;
    }
  }

  return suffix;
}

export function guessContentType(path: string): string {
  if (path.endsWith(".html")) {
    return "text/html; charset=utf-8";
  }

  if (path.endsWith(".css")) {
    return "text/css; charset=utf-8";
  }

  if (path.endsWith(".json")) {
    return "application/json; charset=utf-8";
  }

  if (
    path.endsWith(".js") ||
    path.endsWith(".mjs") ||
    path.endsWith(".cjs") ||
    path.endsWith(".mts") ||
    path.endsWith(".cts")
  ) {
    return "text/javascript; charset=utf-8";
  }

  if (path.endsWith(".ts") || path.endsWith(".tsx") || path.endsWith(".jsx")) {
    return "text/plain; charset=utf-8";
  }

  if (path.endsWith(".md")) {
    return "text/markdown; charset=utf-8";
  }

  if (path.endsWith(".svg")) {
    return "image/svg+xml; charset=utf-8";
  }

  if (path.endsWith(".png")) {
    return "image/png";
  }

  if (path.endsWith(".jpg") || path.endsWith(".jpeg")) {
    return "image/jpeg";
  }

  if (path.endsWith(".gif")) {
    return "image/gif";
  }

  if (path.endsWith(".webp")) {
    return "image/webp";
  }

  if (path.endsWith(".ico")) {
    return "image/x-icon";
  }

  if (path.endsWith(".woff")) {
    return "font/woff";
  }

  if (path.endsWith(".woff2")) {
    return "font/woff2";
  }

  return "application/octet-stream";
}

function isTextContentType(contentType: string): boolean {
  return (
    contentType.startsWith("text/") ||
    contentType.includes("json") ||
    contentType.includes("svg+xml") ||
    contentType.includes("javascript")
  );
}

function decodeText(bytes: Uint8Array): string {
  try {
    return strFromU8(bytes);
  } catch {
    return "";
  }
}
