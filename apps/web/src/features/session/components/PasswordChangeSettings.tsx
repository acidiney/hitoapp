import { useEffect, useId, useState } from "react";

import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import SettingRow from "../../../shared/ui/SettingRow";
import { usePasswordChange } from "../hooks/usePasswordChange";

export default function PasswordChangeSettings({
  account,
  onChanged,
}: {
  account: string;
  onChanged: () => void;
}) {
  const form = usePasswordChange(account, onChanged);
  const [open, setOpen] = useState(false);
  const id = useId();
  useReportUnsaved(form.dirty);
  useEffect(() => {
    if (form.message) setOpen(false);
  }, [form.message]);
  function cancel() {
    form.setCurrent("");
    form.setNext("");
    form.setAgain("");
    setOpen(false);
  }
  const ready =
    form.current !== "" && form.next.length >= 12 && form.next === form.again;
  return (
    <SettingRow
      id="change-password"
      title="Password"
      hint="Changing it signs out every other device."
      action={
        <Button
          onClick={() => {
            form.clearMessage();
            setOpen(true);
          }}
        >
          Change password
        </Button>
      }
    >
      {form.message && <p role="status">{form.message}</p>}
      {open && (
        <Dialog title="Change password" busy={form.busy} onClose={cancel}>
          <form
            className="dialog-form"
            onSubmit={(event) => void form.submit(event)}
          >
            <div className="dialog-field">
              <label htmlFor={`${id}-current`}>Current password</label>
              <input
                id={`${id}-current`}
                type="password"
                autoComplete="current-password"
                required
                disabled={form.busy}
                value={form.current}
                onChange={(event) => form.setCurrent(event.target.value)}
              />
            </div>
            <div className="dialog-field">
              <label htmlFor={`${id}-new`}>New password</label>
              <input
                id={`${id}-new`}
                type="password"
                autoComplete="new-password"
                minLength={12}
                required
                disabled={form.busy}
                value={form.next}
                onChange={(event) => form.setNext(event.target.value)}
              />
              <small>At least 12 characters.</small>
            </div>
            <div className="dialog-field">
              <label htmlFor={`${id}-again`}>Repeat new password</label>
              <input
                id={`${id}-again`}
                type="password"
                autoComplete="new-password"
                required
                disabled={form.busy}
                value={form.again}
                onChange={(event) => form.setAgain(event.target.value)}
              />
            </div>
            <p className="dialog-note">
              Every other device is signed out. This one stays signed in.
            </p>
            {form.error && (
              <p className="form-error" role="alert">
                {form.error}
              </p>
            )}
            <div className="dialog-actions">
              <Button disabled={form.busy} onClick={cancel}>
                Cancel
              </Button>
              <Button
                type="submit"
                variant="action"
                disabled={!ready || form.busy}
              >
                Change password
              </Button>
            </div>
          </form>
        </Dialog>
      )}
    </SettingRow>
  );
}
