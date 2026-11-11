import Switch from "../../../shared/ui/Switch";
import { useAccountSwitch } from "../../../shared/hooks/useAccountSwitch";

export default function LyricsAutoSetting({ account }: { account: string }) {
  const [enabled, setEnabled] = useAccountSwitch("lyrics-auto", account);
  return (
    <Switch
      id="lyrics"
      label="Load lyrics automatically"
      hint="Opening Lyrics asks your first lyrics provider for the playing track."
      checked={enabled}
      onChange={setEnabled}
    />
  );
}
