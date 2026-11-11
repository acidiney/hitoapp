import type { Schema } from "../../../shared/schema";

import type { PluginActions } from "../hooks/usePlugins";
import { usePluginInstallation } from "../hooks/usePluginInstallation";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import PluginInstallationReview from "./PluginInstallationReview";
import PluginTile from "./PluginTile";

export default function ConnectPlugin({
  service,
  actions,
  storageReady,
  onClose,
}: {
  service: Schema<"PluginService">;
  actions: PluginActions;
  storageReady: boolean;
  onClose: () => void;
}) {
  const workflow = usePluginInstallation([service], actions);
  const { token } = workflow;
  const pending = actions.pending || workflow.pending;
  return (
    <Dialog
      title={`Connect ${service.name}`}
      eyebrow="Offered by this server"
      lead={<PluginTile name={service.name} />}
      className="dialog-plugin"
      busy={pending}
      onClose={onClose}
    >
      <form
        className="plugin-form"
        onSubmit={(event) => {
          event.preventDefault();
          void workflow.preview();
        }}
      >
        <p className="plugin-address">{service.base_url}</p>
        <label>
          Bearer token (optional)
          <input
            type="password"
            autoComplete="off"
            maxLength={4096}
            disabled={actions.pending || !storageReady}
            value={token}
            onChange={(event) => workflow.setToken(event.target.value)}
          />
        </label>
        <p>
          Use a token supplied by this service. It stays private to your
          account.
        </p>
        <div className="plugin-actions">
          <Button type="submit" variant="action" disabled={pending}>
            {workflow.pending ? "Checking service…" : "Review connection"}
          </Button>
          <Button type="button" onClick={onClose}>
            Cancel
          </Button>
        </div>
      </form>
      {workflow.error && (
        <p role="alert" className="form-error">
          {workflow.error}
        </p>
      )}
      {workflow.review && (
        <PluginInstallationReview
          manifest={workflow.review.manifest}
          pending={pending}
          onAccept={() => void workflow.accept()}
          onBack={workflow.back}
        />
      )}
    </Dialog>
  );
}
