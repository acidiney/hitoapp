import { pluginInitial, pluginTone } from "../model/pluginIdentity";

export default function PluginTile({
  name,
  size = "card",
}: {
  name: string;
  size?: "card" | "large";
}) {
  const { background, color } = pluginTone(name);
  return (
    <span
      className="plugin-tile"
      data-size={size}
      style={{ background, color }}
      aria-hidden="true"
    >
      {pluginInitial(name)}
    </span>
  );
}
