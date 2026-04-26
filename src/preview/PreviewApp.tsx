import type {
  PreviewDiagnostics,
  PreviewReadyEvent,
  PreviewSelectedFile,
  PreviewWorkspaceFile,
  SessionSnapshot,
} from "../runtime/protocol";

export function PreviewApp(props: {
  diagnostics: PreviewDiagnostics;
  files: PreviewWorkspaceFile[];
  preview: PreviewReadyEvent;
  selectedFile: PreviewSelectedFile | null;
  workspace: SessionSnapshot;
}) {
  const topEntries = props.workspace.archive.entries.slice(0, 8);
  const scripts = Object.entries(props.workspace.packageJson?.scripts ?? {}).slice(0, 4);
  const textFileCount = props.files.length;

  return (
    <div>
      <h3>Preview diagnostics</h3>
      <p>Guest app bootstrap fallback is active. Runtime metadata is shown below.</p>

      <ul>
        <li>
          Command: <code>{props.diagnostics.run.commandLine}</code>
        </li>
        <li>
          Port: <code>{String(props.preview.port)}</code>
        </li>
        <li>
          Workspace: <code>{props.diagnostics.run.cwd}</code>
        </li>
        <li>
          Entrypoint: <code>{props.diagnostics.run.entrypoint}</code>
        </li>
        <li>
          Engine: <code>{props.diagnostics.host.engineName}</code>
        </li>
        <li>
          Text files: <strong>{String(textFileCount)}</strong>
        </li>
      </ul>

      <section>
        <h4>Execution plan</h4>
        <ul>
          <li>
            <code>{props.diagnostics.run.commandKind}</code> {props.diagnostics.run.commandLine}
          </li>
          <li>
            <code>cwd</code> {props.diagnostics.run.cwd}
          </li>
          <li>
            <code>resolved</code> {props.diagnostics.run.resolvedScript ?? "<direct-entry>"}
          </li>
          <li>
            <code>host-vfs</code> {props.diagnostics.hostFiles.count} files / sample{" "}
            {props.diagnostics.hostFiles.samplePath ?? "<none>"}
          </li>
        </ul>
      </section>

      <section>
        <h4>Workspace snapshot</h4>
        <ul>
          {topEntries.map((entry) => (
            <li key={entry.path}>
              <code>{entry.path}</code> {entry.kind}
            </li>
          ))}
        </ul>
      </section>

      <section>
        <h4>Package scripts</h4>
        <ul>
          {scripts.length > 0 ? (
            scripts.map(([name, command]) => (
              <li key={name}>
                <code>{name}</code> {command}
              </li>
            ))
          ) : (
            <li>
              <code>none</code> package.json scripts not detected
            </li>
          )}
        </ul>
      </section>

      <section>
        <h4>Virtual file fetch</h4>
        {props.selectedFile ? (
          <>
            <p>
              <code>{props.selectedFile.path}</code>
            </p>
            <p>
              Preview route: <code>{props.selectedFile.previewUrl}</code>
            </p>
            <p>
              Raw route: <code>{props.selectedFile.url}</code>
            </p>
            <pre>
              <code>{truncateSource(props.selectedFile.content)}</code>
            </pre>
          </>
        ) : (
          <p>No text file was available for preview.</p>
        )}
      </section>

      <section>
        <h4>Diagnostics route</h4>
        <p>
          Hydrated files: <strong>{props.diagnostics.hydratedFileCount}</strong> /{" "}
          <strong>{props.diagnostics.fileCount}</strong>
        </p>
        <p>
          Root hint: <code>{props.diagnostics.rootRequestHint?.kind ?? "none"}</code>
        </p>
        <p>
          Request hint: <code>{props.diagnostics.requestHint?.kind ?? "none"}</code>
        </p>
      </section>
    </div>
  );
}

function truncateSource(source: string): string {
  const lines = source.split("\n").slice(0, 14);
  return lines.join("\n");
}
