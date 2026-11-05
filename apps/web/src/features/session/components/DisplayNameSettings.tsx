import { useId } from "react";

import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { setting } from "../../settings/model/settingsCatalog";
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
    <SettingsCard setting={setting("display-name")} busy={form.busy}>
      <form
        className="display-name-form"
        onSubmit={(event) => void form.submit(event)}
      >
        <fieldset disabled={form.busy}>
          <label htmlFor={`${id}-name`}>Display name</label>
          <input
            id={`${id}-name`}
            type="text"
            autoComplete="nickname"
            placeholder={username}
            value={form.value}
            onChange={(event) => form.setValue(event.target.value)}
          />
          <small>
            Up to 64 characters. You still sign in as {username}. Leave it empty
            to show your username.
          </small>
          {form.message && <p role="status">{form.message}</p>}
          {form.error && (
            <p className="form-error" role="alert">
              {form.error}
            </p>
          )}
          <div>
            <Button type="submit" variant="primary" disabled={!form.dirty}>
              Save display name
            </Button>
          </div>
        </fieldset>
      </form>
    </SettingsCard>
  );
}
