import type {
  PreviewDiagnostics,
  PreviewReadyEvent,
  PreviewWorkspaceFile,
  SessionSnapshot,
} from "../runtime/protocol";

type SelectedPreviewFile = PreviewWorkspaceFile & {
  content: string;
};

export function PreviewApp(props: {
  diagnostics: PreviewDiagnostics;
  files: PreviewWorkspaceFile[];
  preview: PreviewReadyEvent;
  selectedFile: SelectedPreviewFile | null;
  workspace: SessionSnapshot;
}) {
  const { model } = props.preview;
  const topEntries = props.workspace.archive.entries.slice(0, 8);
  const scripts = Object.entries(props.workspace.packageJson?.scripts ?? {}).slice(0, 4);
  const textFileCount = props.files.length;

  return (
    <div className="guest-shell">
      <div className="guest-badge">Guest React mounted</div>
      <h3>{model.title}</h3>
      <p>{model.summary}</p>

      <div className="guest-grid">
        <GuestMetric label="Command" value={model.command} />
        <GuestMetric label="Port" value={String(props.preview.port)} />
        <GuestMetric label="Workspace" value={model.cwd} />
        <GuestMetric label="Entrypoint" value={props.diagnostics.run.entrypoint} />
        <GuestMetric label="Engine" value={props.diagnostics.host.engineName} />
        <GuestMetric label="Text files" value={String(textFileCount)} />
      </div>

      <div className="guest-console">
        {model.highlights.map((item) => (
          <div key={item}>
            <span>$</span>
            <code>{item}</code>
          </div>
        ))}
      </div>

      <div className="guest-columns">
        <section className="guest-card">
          <h4>Execution plan</h4>
          <ul className="guest-list">
            <li>
              <code>{props.diagnostics.run.commandKind}</code>
              <span>{props.diagnostics.run.commandLine}</span>
            </li>
            <li>
              <code>cwd</code>
              <span>{props.diagnostics.run.cwd}</span>
            </li>
            <li>
              <code>resolved</code>
              <span>{props.diagnostics.run.resolvedScript ?? "<direct-entry>"}</span>
            </li>
            <li>
              <code>host-vfs</code>
              <span>
                {props.diagnostics.hostFiles.count} files / sample{" "}
                {props.diagnostics.hostFiles.samplePath ?? "<none>"}
              </span>
            </li>
          </ul>
        </section>

        <section className="guest-card">
          <h4>Workspace snapshot</h4>
          <ul className="guest-list">
            {topEntries.map((entry) => (
              <li key={entry.path}>
                <code>{entry.path}</code>
                <span>{entry.kind}</span>
              </li>
            ))}
          </ul>
        </section>

        <section className="guest-card">
          <h4>Package scripts</h4>
          <ul className="guest-list">
            {scripts.length > 0 ? (
              scripts.map(([name, command]) => (
                <li key={name}>
                  <code>{name}</code>
                  <span>{command}</span>
                </li>
              ))
            ) : (
              <li>
                <code>none</code>
                <span>package.json scripts not detected</span>
              </li>
            )}
          </ul>
        </section>
      </div>

      <section className="guest-card">
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
            <pre className="guest-source">
              <code>{truncateSource(props.selectedFile.content)}</code>
            </pre>
          </>
        ) : (
          <p>No text file was available for preview.</p>
        )}
      </section>

      <section className="guest-card">
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

function GuestMetric(props: { label: string; value: string }) {
  return (
    <div className="guest-metric">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  );
}

function truncateSource(source: string): string {
  const lines = source.split("\n").slice(0, 14);
  return lines.join("\n");
}
