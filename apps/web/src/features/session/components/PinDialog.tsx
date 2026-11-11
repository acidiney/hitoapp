import { useId, type FormEvent } from "react";

import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import type { usePinSettings } from "../hooks/usePinSettings";

export type PinDialogMode = "set" | "change" | "idle" | "remove";

const titles: Record<PinDialogMode, string> = {
  set: "Set a PIN",
  change: "Change PIN",
  idle: "Change the idle lock",
  remove: "Remove the PIN?",
};

export default function PinDialog({
  mode,
  settings,
  onCancel,
}: {
  mode: PinDialogMode;
  settings: ReturnType<typeof usePinSettings>;
  onCancel: () => void;
}) {
  const id = useId();
  const asksPin = mode === "set" || mode === "change";
  const ready =
    settings.password !== "" && (!asksPin || /^[0-9]{4,8}$/.test(settings.pin));
  function remove(event: FormEvent) {
    event.preventDefault();
    settings.remove();
  }
  return (
    <Dialog title={titles[mode]} busy={settings.busy} onClose={onCancel}>
      <form
        className="dialog-form"
        onSubmit={mode === "remove" ? remove : settings.save}
      >
        {asksPin && (
          <div className="dialog-field">
            <label htmlFor={`${id}-pin`}>PIN</label>
            <input
              id={`${id}-pin`}
              type="password"
              inputMode="numeric"
              pattern="[0-9]{4,8}"
              minLength={4}
              maxLength={8}
              autoComplete="off"
              required
              disabled={settings.busy}
              value={settings.pin}
              onChange={(event) => settings.setPin(event.target.value)}
            />
            <small>4 to 8 digits.</small>
          </div>
        )}
        <div className="dialog-field">
          <label htmlFor={`${id}-password`}>Your password</label>
          <input
            id={`${id}-password`}
            type="password"
            autoComplete="current-password"
            required
            disabled={settings.busy}
            value={settings.password}
            onChange={(event) => settings.setPassword(event.target.value)}
          />
          <small>Needed to set, change or remove the PIN.</small>
        </div>
        {mode !== "remove" && (
          <p className="dialog-note">Five wrong PINs sign this device out.</p>
        )}
        {settings.error && (
          <p className="form-error" role="alert">
            {settings.error}
          </p>
        )}
        <div className="dialog-actions">
          <Button disabled={settings.busy} onClick={onCancel}>
            Cancel
          </Button>
          <Button
            type="submit"
            variant={mode === "remove" ? "destructive" : "action"}
            disabled={!ready || settings.busy}
          >
            {mode === "remove" ? "Remove PIN" : "Save PIN"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
