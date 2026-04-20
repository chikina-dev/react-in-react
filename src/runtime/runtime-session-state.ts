import type { PackageJsonSummary, SessionSnapshot } from "./protocol";

export type WorkspaceEntryLike = {
  path: string;
  kind: "file" | "directory";
  size: number;
  isText: boolean;
};

export function applyWorkspaceEntryToSessionSnapshot(
  session: SessionSnapshot,
  entry: WorkspaceEntryLike,
): void {
  const archiveEntry = {
    path: entry.path,
    size: entry.size,
    kind: entry.kind === "directory" ? "dir" : "file",
  } as const;
  const nextEntries = session.archive.entries.filter((candidate) => candidate.path !== entry.path);
  nextEntries.push(archiveEntry);
  nextEntries.sort((left, right) => left.path.localeCompare(right.path));
  session.archive.entries = nextEntries;
  session.archive.fileCount = nextEntries.filter((candidate) => candidate.kind === "file").length;
  session.archive.directoryCount = nextEntries.filter(
    (candidate) => candidate.kind === "dir",
  ).length;
}

export function applyPackageJsonTextToSessionSnapshot(
  session: SessionSnapshot,
  packageJsonText: string | null,
): void {
  const packageJson = parsePackageJsonSummary(packageJsonText);
  session.packageJson = packageJson;

  const dependencies = [
    ...(packageJson?.dependencies ?? []),
    ...(packageJson?.devDependencies ?? []),
  ];
  session.capabilities.detectedReact =
    dependencies.includes("react") || dependencies.includes("react-dom");
  session.capabilities.detectedVite = dependencies.includes("vite");
}

export function parsePackageJsonSummary(packageJsonText: string | null): PackageJsonSummary | null {
  if (!packageJsonText) {
    return null;
  }

  try {
    const parsed = JSON.parse(packageJsonText) as {
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
