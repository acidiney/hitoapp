import { ChevronDown } from "lucide-react";

import ActionMenu from "../../../shared/ui/ActionMenu";
import SettingRow from "../../../shared/ui/SettingRow";
import {
  setStreamingQuality,
  useStreamingQuality,
} from "../browser/StreamingQuality";

const choices = [
  { value: "original", label: "Original" },
  { value: "high", label: "High · AAC 256 kbit/s" },
  { value: "saver", label: "Data saver · AAC 128 kbit/s" },
] as const;

export default function StreamingQualitySettings() {
  const quality = useStreamingQuality();
  const current = choices.find((choice) => choice.value === quality);
  const label = current?.label ?? choices[0].label;
  return (
    <SettingRow
      id="streaming-quality"
      title="Streaming quality"
      hint="Applies when the next track opens. Remote sources keep their own quality."
      action={
        <ActionMenu
          label={`Streaming quality: ${label}`}
          triggerClassName="menu-button"
          align="end"
          icon={
            <>
              {label}
              <ChevronDown size={14} aria-hidden="true" />
            </>
          }
          items={choices.map((choice) => ({
            label: choice.label,
            onSelect: () => setStreamingQuality(choice.value),
          }))}
        />
      }
    />
  );
}
