import { useState, type FormEvent } from "react";
import type { PluginService } from "../../../shared/types";
import type { PluginActions } from "../hooks/usePlugins";

export default function PluginServices({
  services,
  actions,
  onWithdrawn,
}: {
  services: PluginService[];
  actions: PluginActions;
  onWithdrawn: () => void;
}) {
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [confirm, setConfirm] = useState<string | null>(null);
  async function offer(event: FormEvent) {
    event.preventDefault();
    if (
      await actions.offer({
        base_url: address,
        ...(name ? { name } : {}),
      })
    ) {
      setName("");
      setAddress("");
    }
  }
  return (
    <details className="plugin-section plugin-details">
      <summary>Services for this server</summary>
      <p>
        Offer trusted plugin services for accounts to connect. Hito will contact
        these addresses from the server and get their names automatically.
      </p>
      <form className="plugin-form" onSubmit={offer}>
        <label>
          Service address
          <input
            type="url"
            required
            maxLength={2048}
            placeholder="https://music.example/plugins/"
            value={address}
            disabled={actions.pending}
            onChange={(event) => setAddress(event.target.value)}
          />
        </label>
        <details>
          <summary>Advanced settings</summary>
          <label>
            Custom label (optional)
            <input
              maxLength={256}
              value={name}
              disabled={actions.pending}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <p>
            Use a label for services that require a token before sharing their
            name.
          </p>
        </details>
        <button className="button" disabled={actions.pending}>
          Offer service
        </button>
      </form>
      <ul className="plugin-list">
        {services.map((service) => (
          <li key={service.id}>
            <div className="setting-row">
              <div>
                <h3>{service.name}</h3>
                <p className="plugin-address">{service.base_url}</p>
              </div>
              <button
                className="button"
                disabled={actions.pending}
                onClick={() => setConfirm(service.id)}
              >
                Withdraw service
              </button>
            </div>
            {confirm === service.id && (
              <div className="plugin-confirm">
                <p>
                  Withdraw {service.name} for everyone? This disconnects all its
                  installations and clears their saved credentials.
                </p>
                <div className="plugin-actions">
                  <button
                    className="button"
                    disabled={actions.pending}
                    onClick={async () => {
                      if (await actions.withdraw(service)) {
                        setConfirm(null);
                        onWithdrawn();
                      }
                    }}
                  >
                    Withdraw for everyone
                  </button>
                  <button
                    className="button"
                    disabled={actions.pending}
                    onClick={() => setConfirm(null)}
                  >
                    Keep service
                  </button>
                </div>
              </div>
            )}
          </li>
        ))}
      </ul>
    </details>
  );
}
