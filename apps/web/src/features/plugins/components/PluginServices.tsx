import type { Schema } from "../../../shared/schema";
import { useState, type FormEvent } from "react";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";

import type { PluginActions } from "../hooks/usePlugins";

export default function PluginServices({
  services,
  actions,
  onWithdrawn,
}: {
  services: Schema<"PluginService">[];
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
    <SettingsCard
      title="Services for this server"
      description="Offer trusted plugin services for accounts to connect. Hito will contact these addresses from the server and get their names automatically."
    >
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
        <Button type="submit" variant="primary" disabled={actions.pending}>
          Offer service
        </Button>
      </form>
      <ul className="plain-list">
        {services.map((service) => (
          <li key={service.id}>
            <SettingRow
              title={<h4>{service.name}</h4>}
              hint={<span className="plugin-address">{service.base_url}</span>}
              action={
                <Button
                  variant="destructive"
                  disabled={actions.pending}
                  onClick={() => setConfirm(service.id)}
                >
                  Withdraw service
                </Button>
              }
            >
              {confirm === service.id && (
                <div className="plugin-confirm">
                  <p>
                    Withdraw {service.name} for everyone? This disconnects all
                    its installations and clears their saved credentials.
                  </p>
                  <div className="plugin-actions">
                    <Button
                      variant="destructive"
                      disabled={actions.pending}
                      onClick={async () => {
                        if (await actions.withdraw(service)) {
                          setConfirm(null);
                          onWithdrawn();
                        }
                      }}
                    >
                      Withdraw for everyone
                    </Button>
                    <Button
                      disabled={actions.pending}
                      onClick={() => setConfirm(null)}
                    >
                      Keep service
                    </Button>
                  </div>
                </div>
              )}
            </SettingRow>
          </li>
        ))}
      </ul>
    </SettingsCard>
  );
}
