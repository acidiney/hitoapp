import { Play } from "lucide-react";

export default function RowPlay({
  title,
  number,
  disabled,
  onPlay,
}: {
  title: string;
  number: string | number;
  disabled?: boolean;
  onPlay: () => void;
}) {
  return (
    <button
      className="row-play icon-button"
      aria-label={`Play ${title}`}
      disabled={disabled}
      onClick={onPlay}
    >
      <span>{number}</span>
      <Play size={15} />
    </button>
  );
}
