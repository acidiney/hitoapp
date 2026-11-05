import type { Schema } from "../../../shared/schema";

import type { PluginActions } from "../hooks/usePlugins";
import { usePluginInstallation } from "../hooks/usePluginInstallation";
import Button from "../../../shared/ui/Button";
import Listbox from "../../../shared/ui/Listbox";
import SettingsCard from "../../../shared/ui/SettingsCard";
import PluginInstallationReview from "./PluginInstallationReview";
import { setting } from "../../settings/model/settingsCatalog";

export default function ConnectPlugin({
  services,
  actions,
  storageReady,
}: {
  services: Schema<"PluginService">[];
  actions: PluginActions;
  storageReady: boolean;
}) {
  const workflow = usePluginInstallation(services, actions);
  const { service, token } = workflow;
  const pending = actions.pending || workflow.pending;
  if (!service) return null;
  return (
    <SettingsCard setting={setting("connect-service")} busy={pending}>
      <form
        className="plugin-form"
        onSubmit={(event) => {
          event.preventDefault();
          void workflow.preview();
        }}
      >
        <Listbox
          label="Service"
          value={service.id}
          options={services.map((item) => ({
            value: item.id,
            label: item.name,
          }))}
          disabled={actions.pending}
          onChange={workflow.select}
        />
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
        <Button type="submit" variant="primary" disabled={pending}>
          {workflow.pending ? "Checking service…" : "Review connection"}
        </Button>
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
    </SettingsCard>
  );
}
