import type { Schema } from "../../../shared/schema";
import { useEffect, useState } from "react";
import Dialog from "../../../shared/ui/Dialog";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import PlaylistImport from "./PlaylistImport";

type Loaded = {
  connections: Schema<"PluginInstallation">[];
  names: Record<string, string>;
};

export default function PlaylistImportDialog({
  account,
  connections,
  names,
  initialReference,
  onClose,
}: {
  account: string;
  connections?: Schema<"PluginInstallation">[];
  names?: Record<string, string>;
  initialReference?: string;
  onClose: () => void;
}) {
  const [fetched, setLoaded] = useState<Loaded | null>(null);
  const supplied = connections && names ? { connections, names } : null;
  const loaded = supplied ?? fetched;
  useEffect(() => {
    if (supplied) return;
    const controller = new AbortController();
    void pluginsApi
      .settings(account, controller.signal)
      .then((settings) =>
        setLoaded({
          connections: settings.installations,
          names: Object.fromEntries(
            settings.services.map((service) => [service.id, service.name]),
          ),
        }),
      )
      .catch(() => {
        if (!controller.signal.aborted)
          setLoaded({ connections: [], names: {} });
      });
    return () => controller.abort();
  }, [account, supplied]);
  return (
    <Dialog
      title="Import a playlist"
      className="dialog-import"
      onClose={onClose}
    >
      {loaded ? (
        <PlaylistImport
          key={account}
          account={account}
          connections={loaded.connections}
          names={loaded.names}
          initialReference={initialReference}
          onCancel={onClose}
        />
      ) : (
        <p role="status" className="muted">
          Loading your plugins…
        </p>
      )}
    </Dialog>
  );
}
