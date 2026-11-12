import type { Schema } from "../../../shared/schema";
import { useEffect, useState } from "react";
import { ChevronDown } from "lucide-react";

import { useReportUnsaved } from "../../../shared/unsavedChanges";
import ActionMenu from "../../../shared/ui/ActionMenu";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import { useLockSession } from "../hooks/useLockSession";
import { usePinSettings } from "../hooks/usePinSettings";
import { setting } from "../../settings/model/settingsCatalog";
import PinDialog, { type PinDialogMode } from "./PinDialog";

const idleChoices = [
  { minutes: 0, label: "Never" },
  { minutes: 5, label: "5 minutes" },
  { minutes: 15, label: "15 minutes" },
  { minutes: 60, label: "1 hour" },
];

export default function ScreenLockSettings({
  account,
  lock,
}: {
  account: string;
  lock: Schema<"LockStatus">;
}) {
  const settings = usePinSettings(account, lock);
  const player = usePlayer();
  const locking = useLockSession(account, () => player.clear());
  const [dialog, setDialog] = useState<PinDialogMode | null>(null);
  const enabled = settings.status.pin_enabled;
  const savedIdle = settings.status.idle_minutes;
  const current =
    idleChoices.find((choice) => choice.minutes === savedIdle) ??
    idleChoices[0];
  useReportUnsaved(settings.pin !== "" || settings.password !== "");
  useEffect(() => {
    if (settings.message) setDialog(null);
  }, [settings.message]);
  function cancel() {
    settings.setPin("");
    settings.setPassword("");
    settings.setIdleMinutes(savedIdle);
    setDialog(null);
  }
  return (
    <SettingsCard
      setting={setting("screen-lock")}
      description="A PIN locks Hito when you step away. It never replaces your password."
      busy={settings.busy}
    >
      <SettingRow
        id="pin"
        title="PIN"
        hint={enabled ? "On · 4 to 8 digits" : "Off"}
        action={
          enabled ? (
            <>
              <Button onClick={() => setDialog("change")}>Change PIN</Button>
              <Button variant="link" onClick={() => setDialog("remove")}>
                Remove
              </Button>
            </>
          ) : (
            <Button onClick={() => setDialog("set")}>Set a PIN</Button>
          )
        }
      >
        {settings.message && <p role="status">{settings.message}</p>}
      </SettingRow>
      {enabled && (
        <>
          <SettingRow
            id="lock-idle"
            title="Lock when idle"
            action={
              <ActionMenu
                label={`Lock when idle: ${current.label}`}
                triggerClassName="menu-button"
                align="end"
                icon={
                  <>
                    {current.label}
                    <ChevronDown size={14} aria-hidden="true" />
                  </>
                }
                items={idleChoices.map((choice) => ({
                  label: choice.label,
                  onSelect: () => {
                    if (choice.minutes === savedIdle) return;
                    settings.setIdleMinutes(choice.minutes);
                    setDialog("idle");
                  },
                }))}
              />
            }
          />
          <SettingRow
            id="lock-now"
            title="Lock now"
            hint="Stops the music and asks for your PIN."
            action={
              <Button
                disabled={locking.busy}
                onClick={() => void locking.lock()}
              >
                Lock now
              </Button>
            }
          />
        </>
      )}
      {dialog && (
        <PinDialog mode={dialog} settings={settings} onCancel={cancel} />
      )}
    </SettingsCard>
  );
}
