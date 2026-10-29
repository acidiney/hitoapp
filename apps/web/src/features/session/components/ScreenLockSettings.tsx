import { useId } from "react";
import type { LockStatus } from "../../../shared/types";
import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SegmentedControl from "../../../shared/ui/SegmentedControl";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { usePinSettings } from "../hooks/usePinSettings";

const idleChoices = [
  { value: "0", label: "Never" },
  { value: "5", label: "5 min" },
  { value: "15", label: "15 min" },
  { value: "60", label: "1 hour" },
];

export default function ScreenLockSettings({
  account,
  lock,
}: {
  account: string;
  lock: LockStatus;
}) {
  const settings = usePinSettings(account, lock);
  const id = useId();
  const enabled = settings.status.pin_enabled;
  useReportUnsaved(settings.pin !== "" || settings.password !== "");
  return (
    <SettingsCard
      title="Screen lock"
      description="A 4 to 8 digit PIN locks this session while you step away. It never replaces your password: five wrong PINs sign you out, and changing or removing it needs your password."
      busy={settings.busy}
    >
      <form className="pin-form" onSubmit={settings.save}>
        <fieldset disabled={settings.busy}>
          <label htmlFor={`${id}-pin`}>PIN</label>
          <input
            id={`${id}-pin`}
            type="password"
            inputMode="numeric"
            pattern="[0-9]{4,8}"
            minLength={4}
            maxLength={8}
            autoComplete="off"
            required={!enabled}
            value={settings.pin}
            onChange={(event) => settings.setPin(event.target.value)}
          />
          {enabled && <small>Leave blank to keep your current PIN.</small>}
          <span className="field-label">Lock when idle</span>
          <SegmentedControl
            label="Lock when idle"
            options={idleChoices}
            value={String(settings.idleMinutes)}
            onChange={(value) => settings.setIdleMinutes(Number(value))}
            disabled={settings.busy}
          />
          <label htmlFor={`${id}-password`}>Current password</label>
          <input
            id={`${id}-password`}
            type="password"
            autoComplete="current-password"
            required
            value={settings.password}
            onChange={(event) => settings.setPassword(event.target.value)}
          />
          {settings.message && <p role="status">{settings.message}</p>}
          {settings.error && (
            <p className="form-error" role="alert">
              {settings.error}
            </p>
          )}
          <div>
            <Button type="submit" variant="primary">
              Save PIN
            </Button>
            {enabled && (
              <Button
                variant="destructive"
                disabled={settings.password === ""}
                onClick={settings.remove}
              >
                Remove PIN
              </Button>
            )}
          </div>
        </fieldset>
      </form>
    </SettingsCard>
  );
}
