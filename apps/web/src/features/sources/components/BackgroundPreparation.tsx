import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { setting } from "../../settings/model/settingsCatalog";
import { useBackgroundPreparation } from "../hooks/useBackgroundPreparation";

export default function BackgroundPreparation({
  account,
}: {
  account: string;
}) {
  const preparation = useBackgroundPreparation(account);
  const busy = preparation.phase !== null;
  return (
    <SettingsCard setting={setting("background-preparation")} busy={busy}>
      {preparation.phase === "load" && (
        <p role="status">Loading background preparation…</p>
      )}
      {preparation.error && (
        <SectionWarning tone="error" title={preparation.error}>
          {!preparation.loaded && (
            <Button disabled={busy} onClick={preparation.reload}>
              Reload background preparation
            </Button>
          )}
        </SectionWarning>
      )}
      {preparation.loaded && (
        <Switch
          id="background-preparation"
          label="Prepare sources in the background"
          hint="While you're not listening, Hito finds and checks audio for your tracks so Play starts sooner. It never changes a source you chose."
          checked={preparation.enabled}
          disabled={busy}
          onChange={(enabled) => void preparation.setEnabled(enabled)}
        />
      )}
    </SettingsCard>
  );
}
