import type { FormEvent } from "react";
import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { useCommunityRankingPreferences } from "../hooks/useCommunityRankingPreferences";

export default function CommunityRankingSettings({
  account,
}: {
  account: string;
}) {
  const preference = useCommunityRankingPreferences(account);
  const busy = preference.pending !== null;
  useReportUnsaved(preference.dirty);
  function submit(event: FormEvent) {
    event.preventDefault();
    void preference.save();
  }
  return (
    <SettingsCard
      title="Community source rankings"
      description="Use community results to choose among suitable available music sources. Your saved sources and personal source choices keep priority. This requires a configured community service; receiving rankings does not enable playback reporting."
      busy={busy}
    >
      {!preference.loaded && busy && (
        <p role="status">Loading source ranking preference…</p>
      )}
      {preference.error && (
        <SectionWarning tone="error" title={preference.error}>
          <Button disabled={busy} onClick={preference.reload}>
            Reload source ranking preference
          </Button>
        </SectionWarning>
      )}
      {preference.loaded && (
        <form onSubmit={submit}>
          <fieldset disabled={busy}>
            <Switch
              label="Use community source rankings"
              disabled={busy}
              checked={preference.enabled}
              onChange={preference.edit}
            />
            <div className="form-actions">
              <Button
                type="submit"
                variant="primary"
                disabled={busy || !preference.dirty || preference.stale}
              >
                Save source ranking preference
              </Button>
              <Button
                disabled={busy || !preference.dirty}
                onClick={preference.cancel}
              >
                Cancel changes
              </Button>
            </div>
          </fieldset>
          {preference.pending === "save" && (
            <p role="status">Saving source ranking preference…</p>
          )}
          {preference.saved && (
            <p role="status">Source ranking preference saved.</p>
          )}
        </form>
      )}
    </SettingsCard>
  );
}
