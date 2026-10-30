import SettingsCard from "../../../shared/ui/SettingsCard";
import Switch from "../../../shared/ui/Switch";
import { useAccountSwitch } from "../../../shared/hooks/useAccountSwitch";

export default function LyricsAutoSetting({ account }: { account: string }) {
  const [enabled, setEnabled] = useAccountSwitch("lyrics-auto", account);
  return (
    <SettingsCard title="Lyrics">
      <Switch
        label="Load lyrics automatically"
        hint="Opening the Lyrics tab in Now playing asks your first lyrics provider for the playing track. Turn off to load lyrics only when you ask."
        checked={enabled}
        onChange={setEnabled}
      />
    </SettingsCard>
  );
}
