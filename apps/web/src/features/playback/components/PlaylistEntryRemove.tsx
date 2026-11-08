import { Undo2, X } from "lucide-react";

export default function PlaylistEntryRemove({
  title,
  removed,
  disabled,
  onToggle,
}: {
  title: string;
  removed: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  const label = removed
    ? `Keep ${title} in playlist`
    : `Remove ${title} from playlist`;
  return (
    <button
      type="button"
      className="icon-button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onToggle}
    >
      {removed ? <Undo2 size={16} /> : <X size={16} />}
    </button>
  );
}
