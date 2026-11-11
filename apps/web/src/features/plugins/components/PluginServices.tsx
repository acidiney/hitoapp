import type { Schema } from "../../../shared/schema";
import { useState, type FormEvent } from "react";
import Button from "../../../shared/ui/Button";
import { capabilityChips } from "../model/pluginSummary";

import type { PluginActions } from "../hooks/usePlugins";

export default function PluginServices({
  services,
  installations,
  actions,
  error = "",
  errorAction = "",
  onWithdrawn,
}: {
  services: Schema<"PluginService">[];
  installations: Schema<"PluginInstallation">[];
  actions: PluginActions;
  error?: string;
  errorAction?: string;
  onWithdrawn: () => void;
}) {
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [confirm, setConfirm] = useState<string | null>(null);
  const offerError = errorAction === "offer" ? error : "";
  const withdrawError = errorAction === "withdraw" ? error : "";
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
    <div id="setting-plugin-services" className="plugin-admin">
      <p className="plugin-admin-intro">
        What you offer here is what every account on this server can connect.
        Offering a plugin does not connect anyone: each person still chooses for
        themselves under Your plugins.
      </p>
      {services.length > 0 ? (
        <ul className="plugin-offered" aria-label="Services for this server">
          {services.map((service) => {
            const offers = installations
              .filter((plugin) => plugin.service_id === service.id)
              .flatMap((plugin) =>
                capabilityChips(plugin.manifest.capabilities),
              );
            return (
              <li key={service.id}>
                <div className="plugin-offered-name">
                  <h3>{service.name}</h3>
                  <span className="plugin-status" data-status="idle">
                    {offers.length > 0
                      ? `Offered · you use ${[...new Set(offers)].join(", ")}`
                      : "Offered to accounts on this server"}
                  </span>
                </div>
                <span className="plugin-address">{service.base_url}</span>
                <Button
                  variant="destructive"
                  aria-label={`Withdraw ${service.name}`}
                  disabled={actions.pending}
                  onClick={() => setConfirm(service.id)}
                >
                  Withdraw
                </Button>
                {confirm === service.id && (
                  <div className="plugin-confirm">
                    {withdrawError && (
                      <p role="alert" className="plugin-inline-error">
                        {withdrawError}
                      </p>
                    )}
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
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="plugin-admin-intro">
          Nothing is offered yet. Add a service address so accounts on this
          server can connect to it.
        </p>
      )}
      <section className="plugin-offer" aria-labelledby="plugin-offer-title">
        <h2 id="plugin-offer-title">Offer another plugin</h2>
        <form onSubmit={offer}>
          <div className="plugin-offer-row">
            <label>
              Service address
              <input
                type="url"
                required
                maxLength={2048}
                placeholder="https://music.example/plugins/"
                value={address}
                disabled={actions.pending}
                aria-invalid={offerError ? true : undefined}
                aria-describedby={offerError ? "plugin-offer-error" : undefined}
                onChange={(event) => setAddress(event.target.value)}
              />
            </label>
            <Button type="submit" variant="primary" disabled={actions.pending}>
              Offer service
            </Button>
          </div>
          {offerError && (
            <p
              id="plugin-offer-error"
              role="alert"
              className="plugin-inline-error"
            >
              {offerError}
            </p>
          )}
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
        </form>
        <p>
          Hito contacts the address from the server and reads the plugin's name
          and capabilities. A debrid or lyrics service is added the same way.
        </p>
      </section>
    </div>
  );
}
