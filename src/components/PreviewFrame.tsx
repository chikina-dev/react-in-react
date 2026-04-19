import type { PreviewReadyEvent } from "../runtime/protocol";

type PreviewFrameProps = {
  preview: PreviewReadyEvent | null;
  serviceWorkerReady: boolean;
};

export function PreviewFrame(props: PreviewFrameProps) {
  if (!props.preview) {
    return (
      <div className="preview-placeholder">
        <div className="guest-shell">
          <div className="guest-badge">Preview offline</div>
          <h3>セッションを起動するとここに guest React root が描画されます。</h3>
          <p>
            Main Thread は preview URL を受け取り、Service Worker 経由で guest 側の React アプリを
            iframe に表示します。
          </p>
        </div>
      </div>
    );
  }

  if (!props.serviceWorkerReady) {
    return (
      <div className="preview-placeholder">
        <div className="guest-shell">
          <div className="guest-badge">Preview standby</div>
          <h3>Service Worker の準備が整うまで待っています。</h3>
          <p>
            `/preview/&lt;session&gt;/&lt;port&gt;/` を本物の URL として解決するために、 先に
            preview ルーターを登録しています。
          </p>
        </div>
      </div>
    );
  }

  return (
    <iframe
      className="preview-frame"
      sandbox="allow-same-origin allow-scripts"
      src={props.preview.url}
      title="guest-react-preview"
    />
  );
}
