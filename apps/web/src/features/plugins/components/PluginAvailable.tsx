import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import PluginTile from "./PluginTile";

export default function PluginAvailable({
  services,
  admin,
  onConnect,
  onOffer,
}: {
  services: Schema<"PluginService">[];
  admin: boolean;
  onConnect: (service: Schema<"PluginService">) => void;
  onOffer: () => void;
}) {
  if (services.length === 0) return null;
  return (
    <section
      id="setting-connect-service"
      className="plugin-section"
      aria-labelledby="plugin-available-title"
    >
      <h2 id="plugin-available-title" tabIndex={-1}>
        Available on this server
      </h2>
      <ul className="plugin-available">
        {services.map((service) => (
          <li key={service.id}>
            <div className="plugin-card-head">
              <PluginTile name={service.name} />
              <div className="plugin-card-title">
                <h3>{service.name}</h3>
                <span className="plugin-status" data-status="idle">
                  Not connected
                </span>
              </div>
            </div>
            <Button variant="action" onClick={() => onConnect(service)}>
              Connect {service.name}
            </Button>
          </li>
        ))}
      </ul>
      <p className="plugin-note">
        Connections are yours only. A plugin that needs a sign-in brings you
        back here.
      </p>
      {admin && (
        <p className="plugin-note">
          <Button onClick={onOffer}>Offer a plugin to this server</Button>
        </p>
      )}
    </section>
  );
}

export function PluginNothingOffered({
  admin,
  onOffer,
}: {
  admin: boolean;
  onOffer: () => void;
}) {
  return (
    <section
      className="plugin-section plugin-section-tight"
      aria-labelledby="plugin-empty-title"
    >
      <h2 id="plugin-empty-title">Nothing offered here yet</h2>
      <div className="plugin-dashed">
        <div>
          <strong>No plugins are available on this server.</strong>
          <span>
            {admin
              ? "Offer a plugin so accounts on this server can connect to it."
              : "Your administrator has not offered any services yet. Only an administrator can add one."}
          </span>
        </div>
        {admin && (
          <Button onClick={onOffer}>Offer a plugin to this server</Button>
        )}
      </div>
    </section>
  );
}
