import { Link } from "@inertiajs/react";
import { Plug } from "lucide-react";

export default function PluginsPill() {
  return (
    <Link href="/settings?section=plugins" className="plugins-pill">
      <Plug size={16} aria-hidden="true" />
      Plugins
    </Link>
  );
}
