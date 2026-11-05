import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import HealthDot from "../../../shared/ui/HealthDot";
import SettingRow from "../../../shared/ui/SettingRow";
import {
  usePluginAuthorization,
  type PluginAuthorizationActions,
} from "../hooks/usePluginAuthorization";
import { grantedAccess } from "../model/pluginSummary";

type Tone = "success" | "warning" | "action";

function summary(
  provider: string,
  authorization: PluginAuthorizationActions,
): { label: string; tone: Tone } {
  const { phase, connection, checkFailed } = authorization;
  if (phase.kind === "starting" || phase.kind === "waiting")
    return { label: `Waiting for ${provider}…`, tone: "warning" };
  if (!connection)
    return checkFailed
      ? { label: "Sign-in status unavailable", tone: "warning" }
      : { label: "Checking sign-in…", tone: "warning" };
  if (connection.status === "connected")
    return { label: "Connected", tone: "success" };
  if (connection.status === "needs_reauthorization")
    return { label: "Sign in again needed", tone: "warning" };
  return { label: "Not connected", tone: "action" };
}

function Actions({
  authorization,
  onDisconnect,
}: {
  authorization: PluginAuthorizationActions;
  onDisconnect: () => void;
}) {
  const { phase, connection, checkFailed } = authorization;
  if (phase.kind === "waiting")
    return <Button onClick={authorization.stopWaiting}>Stop waiting</Button>;
  const busy = phase.kind !== "idle";
  if (!connection)
    return checkFailed ? (
      <Button onClick={authorization.check}>Check again</Button>
    ) : null;
  return (
    <>
      {connection.status !== "connected" && (
        <Button
          variant="primary"
          disabled={busy}
          onClick={() => void authorization.connect()}
        >
          {connection.status === "needs_reauthorization"
            ? "Reconnect"
            : "Connect"}
        </Button>
      )}
      {connection.status !== "not_connected" && (
        <Button disabled={busy} onClick={onDisconnect}>
          Disconnect
        </Button>
      )}
    </>
  );
}

export default function PluginAuthorization({
  account,
  plugin,
}: {
  account: string;
  plugin: Schema<"PluginInstallation">;
}) {
  const authorization = usePluginAuthorization(account, plugin);
  const [confirming, setConfirming] = useState(false);
  if (!authorization.enabled) return null;
  const provider = plugin.manifest.name;
  const { phase, connection, notice } = authorization;
  const { label, tone } = summary(provider, authorization);
  const access =
    connection?.status === "connected" ? grantedAccess(connection.scopes) : "";
  const attempt = phase.kind === "waiting" ? phase.attempt : null;
  return (
    <div className="plugin-authorization">
      <SettingRow
        title={<h5>Account sign-in</h5>}
        hint={
          attempt ? (
            <>
              Finish signing in on the {provider} page. This page updates by
              itself.
            </>
          ) : access ? (
            `Access: ${access}`
          ) : (
            `Sign in with ${provider} so Hito can use your account there.`
          )
        }
        status={
          <span role="status" aria-label="Sign-in status">
            <HealthDot tone={tone} label={label} />
          </span>
        }
        wide
        action={
          <Actions
            authorization={authorization}
            onDisconnect={() => setConfirming(true)}
          />
        }
      />
      {attempt && !attempt.opened && (
        <p className="plugin-authorization-link">
          Your browser did not open a new tab.{" "}
          <a href={attempt.url} target="_blank" rel="noopener noreferrer">
            Open {provider} sign-in
          </a>
        </p>
      )}
      {notice && (
        <p role={notice.tone === "error" ? "alert" : "status"}>{notice.text}</p>
      )}
      {confirming && (
        <Dialog
          title={`Disconnect ${provider}?`}
          busy={phase.kind === "disconnecting"}
          onClose={() => setConfirming(false)}
        >
          <p className="muted">
            Hito will delete the saved sign-in for {provider} on this account.
            You can connect again at any time.
          </p>
          <div className="dialog-actions">
            <Button onClick={() => setConfirming(false)}>Cancel</Button>
            <Button
              variant="destructive"
              onClick={async () => {
                await authorization.disconnect();
                setConfirming(false);
              }}
            >
              Disconnect
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
