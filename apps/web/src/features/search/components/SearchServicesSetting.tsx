import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { useAutomaticServiceSearch } from "../model/autoSearch";

export default function SearchServicesSetting({ account }: { account: string }) {
  const [enabled, setEnabled] = useAutomaticServiceSearch(account);
  return (
    <SettingsCard title="Search">
      <Switch
        label="Search connected services automatically"
        hint="When your library has few matches, the search text is sent to your connected music services. Turn off to send it only when you ask."
        checked={enabled}
        onChange={setEnabled}
      />
    </SettingsCard>
  );
}
