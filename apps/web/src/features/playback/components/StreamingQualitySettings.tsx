import SettingsCard from "../../../shared/ui/SettingsCard";
import SettingRow from "../../../shared/ui/SettingRow";
import { setting } from "../../settings/model/settingsCatalog";
import {
  setStreamingQuality,
  useStreamingQuality,
} from "../browser/StreamingQuality";

export default function StreamingQualitySettings() {
  const quality = useStreamingQuality();
  return (
    <SettingsCard setting={setting("streaming-quality")}>
      <SettingRow
        title={<label htmlFor="streaming-quality">Streaming quality</label>}
        hint="Applies to this browser when audio next opens. The current track keeps playing. Local files and saved copies can use AAC; remote sources keep their original quality."
        action={
          <select
            id="streaming-quality"
            value={quality}
            onChange={(event) => {
              const value = event.currentTarget.value;
              setStreamingQuality(
                value === "high" || value === "saver" ? value : "original",
              );
            }}
          >
            <option value="original">Original</option>
            <option value="high">High · AAC 256 kbit/s</option>
            <option value="saver">Data saver · AAC 128 kbit/s</option>
          </select>
        }
      />
    </SettingsCard>
  );
}
