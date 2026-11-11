import { useReportUnsaved } from "../../../shared/unsavedChanges";
import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
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
  return (
    <>
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
        <>
          <Switch
            id="community-rankings"
            label="Use community source rankings"
            hint="Helps choose between sources. Your own choices always win."
            disabled={busy}
            checked={preference.enabled}
            onChange={preference.edit}
          />
          {preference.dirty && (
            <div className="settings-save">
              <Button
                variant="action"
                disabled={busy || preference.stale}
                onClick={() => void preference.save()}
              >
                Save source ranking preference
              </Button>
              <Button disabled={busy} onClick={preference.cancel}>
                Cancel changes
              </Button>
            </div>
          )}
          {preference.pending === "save" && (
            <p role="status">Saving source ranking preference…</p>
          )}
          {preference.saved && (
            <p role="status">Source ranking preference saved.</p>
          )}
        </>
      )}
    </>
  );
}
