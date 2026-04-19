import type { PreviewReadyEvent, PreviewWorkspaceFile, SessionSnapshot } from "../runtime/protocol";

type SelectedPreviewFile = PreviewWorkspaceFile & {
  content: string;
};

export function PreviewApp(props: {
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
        <GuestMetric label="Renderer" value="Service Worker preview route" />
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
