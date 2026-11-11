import type { Schema } from "../../../shared/schema";
import { useRef, useState } from "react";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import Skeleton from "../../../shared/ui/Skeleton";
import { usePlugins } from "../hooks/usePlugins";
import ConnectPlugin from "./ConnectPlugin";
import PluginAvailable, { PluginNothingOffered } from "./PluginAvailable";
import PluginConnection from "./PluginConnection";
import PluginPlaybackOrder from "./PluginPlaybackOrder";
import PluginPower from "./PluginPower";
import PluginServices from "./PluginServices";

type View = "mine" | "server";

export default function PluginSettings(props: {
  account: string;
  admin: boolean;
  desktop?: boolean;
}) {
  return <PluginPanel key={props.account} {...props} />;
}

function PluginTabs({
  view,
  onChange,
}: {
  view: View;
  onChange: (view: View) => void;
}) {
  return (
    <nav aria-label="Plugin views" className="plugin-tabs">
      <button
        type="button"
        aria-current={view === "mine" ? "page" : undefined}
        onClick={() => onChange("mine")}
      >
        Your plugins
      </button>
      <button
        type="button"
        aria-current={view === "server" ? "page" : undefined}
        onClick={() => onChange("server")}
      >
        <span>This server</span>
        <span className="plugin-tab-badge">Admin</span>
      </button>
    </nav>
  );
}

function PluginPanel({
  account,
  admin,
  desktop = false,
}: {
  account: string;
  admin: boolean;
  desktop?: boolean;
}) {
  const actions = usePlugins(account);
  const heading = useRef<HTMLHeadingElement>(null);
  const [view, setView] = useState<View>("mine");
  const [connecting, setConnecting] = useState<string | null>(null);
  const restoreFocus = () => heading.current?.focus();
  const { data, error, errorAction, pending } = actions;
  const serverError =
    errorAction === "offer" || errorAction === "withdraw" ? error : "";
  const pageError = serverError ? "" : error;
  const available =
    data?.services.filter(
      (service) =>
        !data.installations.some((plugin) => plugin.service_id === service.id),
    ) ?? [];
  const connectingService = available.find((item) => item.id === connecting);
  if (connecting && data && !connectingService) setConnecting(null);
  const showServer = admin && view === "server";
  const fresh = data && data.installations.length === 0;
  return (
    <div
      className="plugins-page"
      data-fresh={fresh || undefined}
      aria-busy={pending}
    >
      <header className="plugins-head">
        <div>
          <h1 ref={heading} tabIndex={-1}>
            Plugins
          </h1>
          {!showServer && (
            <p>
              {fresh
                ? "Your library plays local files on its own. Connect a plugin to search, play and follow music you don't have files for."
                : "Each plugin adds capabilities. You decide which stay on."}
            </p>
          )}
        </div>
        {admin && <PluginTabs view={view} onChange={setView} />}
      </header>
      {pending && (
        <p role="status" className="plugin-working">
          Working on plugin connection…
        </p>
      )}
      {pageError && (
        <SectionWarning tone="error" title={pageError}>
          <Button disabled={pending} onClick={actions.reload}>
            Reload connections
          </Button>
        </SectionWarning>
      )}
      {!data && !error && (
        <>
          <p role="status" className="sr-only">
            Loading plugin connections…
          </p>
          <Skeleton rows={3} />
        </>
      )}
      {data && showServer && (
        <PluginServices
          services={data.services}
          installations={data.installations}
          actions={actions}
          error={serverError}
          errorAction={errorAction}
          onWithdrawn={restoreFocus}
        />
      )}
      {data && !showServer && (
        <>
          {!data.credential_storage_ready && (
            <SectionWarning title="Saved credentials are unavailable.">
              Ask your administrator to restore the plugin key and restart Hito.
              You can still clear tokens and remove connections.
            </SectionWarning>
          )}
          {!fresh && <PluginPower installations={data.installations} />}
          <div className="plugins-columns">
            <div className="plugins-main">
              {data.installations.length > 0 && (
                <section
                  id="setting-your-plugins"
                  className="plugin-section"
                  aria-labelledby="plugin-connections-title"
                >
                  <h2 id="plugin-connections-title">Your connections</h2>
                  <ul className="plugin-grid">
                    {data.installations.map((plugin) => (
                      <PluginConnection
                        key={plugin.id}
                        account={account}
                        plugin={plugin}
                        actions={actions}
                        onRemoved={restoreFocus}
                        storageReady={data.credential_storage_ready}
                        desktop={desktop}
                      />
                    ))}
                  </ul>
                </section>
              )}
              {available.length > 0 ? (
                <PluginAvailable
                  services={available}
                  admin={admin}
                  onConnect={(service) => setConnecting(service.id)}
                  onOffer={() => setView("server")}
                />
              ) : (
                data.installations.length === 0 && (
                  <PluginNothingOffered
                    admin={admin}
                    onOffer={() => setView("server")}
                  />
                )
              )}
              {available.length === 0 && data.installations.length > 0 && (
                <section
                  className="plugin-section plugin-section-tight"
                  aria-labelledby="plugin-done-title"
                >
                  <h2 id="plugin-done-title">Available on this server</h2>
                  <div className="plugin-dashed">
                    <div>
                      <strong>
                        You've connected every plugin this server offers
                      </strong>
                      <span>
                        New plugins appear here once an administrator offers
                        them.
                      </span>
                    </div>
                    {admin && (
                      <Button onClick={() => setView("server")}>
                        Offer a plugin to this server
                      </Button>
                    )}
                  </div>
                </section>
              )}
            </div>
            {data.installations.length > 0 && (
              <PluginPlaybackOrder account={account} />
            )}
          </div>
        </>
      )}
      {data && connectingService && (
        <ConnectPlugin
          key={connectingService.id}
          service={connectingService}
          actions={actions}
          storageReady={data.credential_storage_ready}
          onClose={() => setConnecting(null)}
        />
      )}
    </div>
  );
}
