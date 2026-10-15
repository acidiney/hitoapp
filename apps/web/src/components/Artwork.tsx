import { useState } from "react";
import { Disc3, ListMusic } from "lucide-react";

export default function Artwork({
  recording,
  playlist = false,
  className = "",
}: {
  recording?: string;
  playlist?: boolean;
  className?: string;
}) {
  const [failed, setFailed] = useState<string>();
  const image = recording && failed !== recording;
  return (
    <div
      className={`artwork ${image ? "has-image" : ""} ${className}`}
      aria-hidden="true"
    >
      {image ? (
        <img
          src={`/api/v1/recordings/${encodeURIComponent(recording)}/artwork`}
          alt=""
          loading="lazy"
          onError={() => setFailed(recording)}
        />
      ) : playlist ? (
        <ListMusic strokeWidth={1} />
      ) : (
        <Disc3 strokeWidth={0.8} />
      )}
    </div>
  );
}
