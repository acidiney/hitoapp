import { createRoot } from "react-dom/client";
import Button from "../shared/ui/Button";
import "../styles/update-notice.css";

export default function UpdateNotice({
  onReload,
  onDismiss,
}: {
  onReload: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="update-notice" role="status">
      <span>A new version of Hito is ready</span>
      <Button variant="action" onClick={onReload}>
        Reload
      </Button>
      <Button variant="link" onClick={onDismiss}>
        Later
      </Button>
    </div>
  );
}

export function showUpdateNotice(apply: () => void) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const dismiss = () => {
    root.unmount();
    host.remove();
  };
  root.render(<UpdateNotice onReload={apply} onDismiss={dismiss} />);
}
