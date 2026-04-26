import type { PackageJsonSummary, SessionSnapshot } from "./protocol";
import type { RunRequest } from "./protocol";

const SUGGESTED_RUN_SCRIPT_NAMES = ["dev", "start"] as const;

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
  session.suggestedRunRequest = deriveSuggestedRunRequest(packageJson, session.workspaceRoot);

  const dependencies = [
    ...(packageJson?.dependencies ?? []),
    ...(packageJson?.devDependencies ?? []),
  ];
  session.capabilities.detectedReact =
    dependencies.includes("react") || dependencies.includes("react-dom");
}

export function deriveSuggestedRunRequest(
  packageJson: PackageJsonSummary | null,
  workspaceRoot = "/workspace",
): RunRequest | null {
  const scriptName = deriveSuggestedRunScriptName(packageJson);
  if (scriptName == null) {
    return null;
  }

  return {
    cwd: workspaceRoot,
    command: "npm",
    args: ["run", scriptName],
  };
}

export function deriveSuggestedRunScriptName(
  packageJson: PackageJsonSummary | null,
): (typeof SUGGESTED_RUN_SCRIPT_NAMES)[number] | null {
  const scripts = packageJson?.scripts ?? {};

  for (const scriptName of SUGGESTED_RUN_SCRIPT_NAMES) {
    const candidate = scripts[scriptName];
    if (typeof candidate === "string" && candidate.trim().length > 0) {
      return scriptName;
    }
  }

  return null;
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
