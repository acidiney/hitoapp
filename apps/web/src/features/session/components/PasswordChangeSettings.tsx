import { useId } from "react";

import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { usePasswordChange } from "../hooks/usePasswordChange";
import { setting } from "../../settings/model/settingsCatalog";

export default function PasswordChangeSettings({
  account,
  onChanged,
}: {
  account: string;
  onChanged: () => void;
}) {
  const form = usePasswordChange(account, onChanged);
  const id = useId();
  useReportUnsaved(form.dirty);
  return (
    <SettingsCard setting={setting("change-password")} busy={form.busy}>
      <form
        className="password-form"
        onSubmit={(event) => void form.submit(event)}
      >
        <fieldset disabled={form.busy}>
          <label htmlFor={`${id}-current`}>Current password</label>
          <input
            id={`${id}-current`}
            type="password"
            autoComplete="current-password"
            required
            value={form.current}
            onChange={(event) => form.setCurrent(event.target.value)}
          />
          <label htmlFor={`${id}-new`}>New password</label>
          <input
            id={`${id}-new`}
            type="password"
            autoComplete="new-password"
            minLength={12}
            required
            value={form.next}
            onChange={(event) => form.setNext(event.target.value)}
          />
          <small>Use at least 12 characters.</small>
          <label htmlFor={`${id}-again`}>Repeat new password</label>
          <input
            id={`${id}-again`}
            type="password"
            autoComplete="new-password"
            required
            value={form.again}
            onChange={(event) => form.setAgain(event.target.value)}
          />
          {form.message && <p role="status">{form.message}</p>}
          {form.error && (
            <p className="form-error" role="alert">
              {form.error}
            </p>
          )}
          <div>
            <Button type="submit" variant="primary">
              Change password
            </Button>
          </div>
        </fieldset>
      </form>
    </SettingsCard>
  );
}
