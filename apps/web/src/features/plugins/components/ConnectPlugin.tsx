import type { PluginService } from "../../../shared/types";
import type { PluginActions } from "../hooks/usePlugins";
import { usePluginInstallation } from "../hooks/usePluginInstallation";
import PluginInstallationReview from "./PluginInstallationReview";

export default function ConnectPlugin({
  services,
  actions,
  storageReady,
}: {
  services: PluginService[];
  actions: PluginActions;
  storageReady: boolean;
}) {
  const workflow = usePluginInstallation(services, actions);
  const { service, token } = workflow;
  const pending = actions.pending || workflow.pending;
  if (!service) return null;
  return (
    <div className="plugin-section">
      <h3>Connect a service</h3>
      <form
        className="plugin-form"
        onSubmit={(event) => {
          event.preventDefault();
          void workflow.preview();
        }}
      >
        <label>
          Service
          <select
            value={service.id}
            disabled={actions.pending}
            onChange={(event) => {
              workflow.select(event.target.value);
            }}
          >
            {services.map((item) => (
              <option key={item.id} value={item.id}>
                {item.name}
              </option>
            ))}
          </select>
        </label>
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
        <button className="button primary" disabled={pending}>
          {workflow.pending ? "Checking service…" : "Review connection"}
        </button>
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
    </div>
  );
}
