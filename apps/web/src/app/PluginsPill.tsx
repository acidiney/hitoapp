import { Link } from "@inertiajs/react";
import { Plug } from "lucide-react";
import {
  pluginCountsLabel,
  type PluginCounts,
} from "../features/plugins/model/pluginCounts";

export default function PluginsPill({
  counts,
}: {
  counts: PluginCounts | null;
}) {
  return (
    <Link
      href="/settings?section=plugins"
      className="plugins-pill"
      aria-label={counts ? pluginCountsLabel(counts) : "Plugins"}
    >
      <Plug size={16} aria-hidden="true" />
      <span>
        {counts
          ? `${counts.installed} ${counts.installed === 1 ? "plugin" : "plugins"}`
          : "Plugins"}
      </span>
      {counts !== null && counts.review > 0 && (
        <span className="plugins-pill-review">· {counts.review} to review</span>
      )}
    </Link>
  );
}
