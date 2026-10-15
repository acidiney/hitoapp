import { useState } from "react";
import { Disc3, ListMusic } from "lucide-react";

export default function Artwork({
  recording,
  release,
  playlist = false,
  className = "",
}: {
  recording?: string;
  release?: string;
  playlist?: boolean;
  className?: string;
}) {
  const [failed, setFailed] = useState<string>();
  const url = release
    ? `/api/v1/releases/${encodeURIComponent(release)}/artwork`
    : recording
      ? `/api/v1/recordings/${encodeURIComponent(recording)}/artwork`
      : undefined;
  const image = url && failed !== url;
  return (
    <div
      className={`artwork ${image ? "has-image" : ""} ${className}`}
      aria-hidden="true"
    >
      {image ? (
        <img src={url} alt="" loading="lazy" onError={() => setFailed(url)} />
      ) : playlist ? (
        <ListMusic strokeWidth={1} />
      ) : (
        <Disc3 strokeWidth={0.8} />
      )}
    </div>
  );
}
