import { useState, type FormEvent } from "react";
import type { PluginService } from "../../types";
import type { PluginActions } from "../../hooks/usePlugins";

export default function ConnectPlugin({
  services,
  actions,
  storageReady,
}: {
  services: PluginService[];
  actions: PluginActions;
  storageReady: boolean;
}) {
  const [selected, setSelected] = useState(services[0]?.id ?? "");
  const [token, setToken] = useState("");
  const service = services.find((item) => item.id === selected) ?? services[0];
  async function connect(event: FormEvent) {
    event.preventDefault();
    if (
      service &&
      (await actions.install({
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: token || null,
      }))
    )
      setToken("");
  }
  if (!service) return null;
  return (
    <div className="plugin-section">
      <h3>Connect a service</h3>
      <form className="plugin-form" onSubmit={connect}>
        <label>
          Service
          <select
            value={service.id}
            disabled={actions.pending}
            onChange={(event) => {
              setSelected(event.target.value);
              setToken("");
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
            onChange={(event) => setToken(event.target.value)}
          />
        </label>
        <p>
          Use a token supplied by this service. It stays private to your
          account.
        </p>
        <button className="button primary" disabled={actions.pending}>
          {actions.pending ? "Connecting…" : "Connect service"}
        </button>
      </form>
    </div>
  );
}
