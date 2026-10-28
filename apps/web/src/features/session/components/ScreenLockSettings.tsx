import { useId } from "react";
import type { LockStatus } from "../../../shared/types";
import { usePinSettings } from "../hooks/usePinSettings";

const idleChoices = [
  { value: 0, label: "Never" },
  { value: 5, label: "After 5 minutes" },
  { value: 15, label: "After 15 minutes" },
  { value: 60, label: "After 1 hour" },
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
  return (
    <section className="settings-panel" aria-label="Screen lock">
      <div className="settings-intro">
        <h2>Screen lock</h2>
        <p>
          Set a 4 to 8 digit PIN to lock this session while you step away. The
          PIN never replaces your password: five wrong PINs sign you out, and
          changing or removing the PIN needs your password.
        </p>
      </div>
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
          <label htmlFor={`${id}-idle`}>Lock when idle</label>
          <select
            id={`${id}-idle`}
            value={settings.idleMinutes}
            onChange={(event) =>
              settings.setIdleMinutes(Number(event.target.value))
            }
          >
            {idleChoices.map((choice) => (
              <option key={choice.value} value={choice.value}>
                {choice.label}
              </option>
            ))}
          </select>
          <label htmlFor={`${id}-password`}>Current password</label>
          <input
            id={`${id}-password`}
            type="password"
            autoComplete="current-password"
            required
            value={settings.password}
            onChange={(event) => settings.setPassword(event.target.value)}
          />
          {settings.error && (
            <p className="form-error" role="alert">
              {settings.error}
            </p>
          )}
          <div>
            <button className="button primary" type="submit">
              Save PIN
            </button>
            {enabled && (
              <button
                className="button"
                type="button"
                disabled={settings.password === ""}
                onClick={settings.remove}
              >
                Remove PIN
              </button>
            )}
          </div>
        </fieldset>
      </form>
    </section>
  );
}
