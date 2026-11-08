import { useRef } from "react";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { usePlugins } from "../hooks/usePlugins";
import ConnectPlugin from "./ConnectPlugin";
import PluginConnection from "./PluginConnection";
import PluginServices from "./PluginServices";
import { setting } from "../../settings/model/settingsCatalog";

export default function PluginSettings(props: {
  account: string;
  admin: boolean;
  desktop?: boolean;
}) {
  return <PluginPanel key={props.account} {...props} />;
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
  const connectHeading = useRef<HTMLHeadingElement>(null);
  const restoreFocus = () => heading.current?.focus();
  const { data, error, pending } = actions;
  const available =
    data?.services.filter(
      (service) =>
        !data.installations.some((plugin) => plugin.service_id === service.id),
    ) ?? [];
  return (
    <>
      <SettingsCard
        setting={setting("your-plugins")}
        headingRef={heading}
        busy={pending}
        action={
          available.length > 0 && (
            <Button
              onClick={() => {
                connectHeading.current?.scrollIntoView?.({ block: "start" });
                connectHeading.current?.focus({ preventScroll: true });
              }}
            >
              Connect a service
            </Button>
          )
        }
      >
        {pending && <p role="status">Working on plugin connection…</p>}
        {error && (
          <SectionWarning tone="error" title={error}>
            <Button disabled={pending} onClick={actions.reload}>
              Reload connections
            </Button>
          </SectionWarning>
        )}
        {!data && !error && <p role="status">Loading plugin connections…</p>}
        {data && (
          <>
            {!data.credential_storage_ready && (
              <SectionWarning title="Saved credentials are unavailable.">
                Ask your administrator to restore the plugin key and restart
                Hito. You can still clear tokens and remove connections.
              </SectionWarning>
            )}
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
            {!data.installations.length && (
              <p>
                No plugins connected to this account.
                {!data.services.length &&
                  !admin &&
                  " Your administrator has not offered any services yet."}
              </p>
            )}
          </>
        )}
      </SettingsCard>
      {data && (
        <>
          <ConnectPlugin
            services={available}
            actions={actions}
            storageReady={data.credential_storage_ready}
            headingRef={connectHeading}
          />
          {admin && (
            <PluginServices
              services={data.services}
              actions={actions}
              onWithdrawn={restoreFocus}
            />
          )}
        </>
      )}
    </>
  );
}
