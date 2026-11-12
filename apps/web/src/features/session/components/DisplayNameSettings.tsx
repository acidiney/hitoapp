import { useId } from "react";

import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import { useDisplayName } from "../hooks/useDisplayName";

export default function DisplayNameSettings({
  account,
  username,
  displayName,
  onSaved,
}: {
  account: string;
  username: string;
  displayName: string | null;
  onSaved: () => void;
}) {
  const form = useDisplayName(account, displayName, onSaved);
  const id = useId();
  useReportUnsaved(form.dirty);
  return (
    <form onSubmit={(event) => void form.submit(event)}>
      <SettingRow
        id="display-name"
        title={<label htmlFor={`${id}-name`}>Display name</label>}
        hint={`Shown instead of your username. You still sign in as ${username}.`}
        action={
          <>
            <input
              id={`${id}-name`}
              type="text"
              autoComplete="nickname"
              placeholder={username}
              disabled={form.busy}
              value={form.value}
              onChange={(event) => form.setValue(event.target.value)}
            />
            <Button
              type="submit"
              variant="action"
              aria-label="Save display name"
              disabled={!form.dirty || form.busy}
            >
              Save
            </Button>
          </>
        }
      >
        {form.message && <p role="status">{form.message}</p>}
        {form.error && (
          <p className="form-error" role="alert">
            {form.error}
          </p>
        )}
      </SettingRow>
    </form>
  );
}
