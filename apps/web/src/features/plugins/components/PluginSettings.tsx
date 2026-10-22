import { useRef } from "react";
import { Link } from "@inertiajs/react";
import { usePlugins } from "../hooks/usePlugins";
import ConnectPlugin from "./ConnectPlugin";
import PluginConnection from "./PluginConnection";
import PluginServices from "./PluginServices";

export default function PluginSettings(props: {
  account: string;
  admin: boolean;
}) {
  return <PluginPanel key={props.account} {...props} />;
}
function PluginPanel({ account, admin }: { account: string; admin: boolean }) {
  const actions = usePlugins(account);
  const heading = useRef<HTMLHeadingElement>(null);
  const restoreFocus = () => heading.current?.focus();
  const { data, error, pending } = actions;
  const available =
    data?.services.filter(
      (service) =>
        !data.installations.some((plugin) => plugin.service_id === service.id),
    ) ?? [];
  return (
    <section
      className="settings-panel"
      aria-labelledby="plugins-title"
      aria-busy={pending}
    >
      <div className="settings-intro">
        <h2 id="plugins-title" ref={heading} tabIndex={-1}>
          Your plugins
        </h2>
        <p>
          Connect to services offered by your administrator. Each account keeps
          its own connections and credentials.
        </p>
        <p>
          Search connected music services in{" "}
          <Link className="text-link" href="/discover">
            Discover
          </Link>
          .
        </p>
      </div>
      {pending && <p role="status">Working on plugin connection…</p>}
      {error && (
        <div role="alert" className="form-error">
          <p>{error}</p>
          <button
            className="button"
            disabled={pending}
            onClick={actions.reload}
          >
            Reload connections
          </button>
        </div>
      )}
      {!data && !error && <p role="status">Loading plugin connections…</p>}
      {data && (
        <>
          {!data.credential_storage_ready && (
            <p role="status">
              Saved credentials are unavailable. Ask your administrator to
              restore the plugin key and restart Hito. You can still clear
              tokens and remove connections.
            </p>
          )}
          <ul className="plugin-list">
            {data.installations.map((plugin) => (
              <PluginConnection
                key={plugin.id}
                plugin={plugin}
                actions={actions}
                onRemoved={restoreFocus}
                storageReady={data.credential_storage_ready}
              />
            ))}
          </ul>
          {!data.installations.length && (
            <p>No plugins connected to this account.</p>
          )}
          <ConnectPlugin
            services={available}
            actions={actions}
            storageReady={data.credential_storage_ready}
          />
          {!data.services.length && (
            <p>
              {admin
                ? "Offer a service below to make it available to accounts on this server."
                : "Your administrator has not offered any services yet."}
            </p>
          )}
          {admin && (
            <PluginServices
              services={data.services}
              actions={actions}
              onWithdrawn={restoreFocus}
            />
          )}
        </>
      )}
    </section>
  );
}
