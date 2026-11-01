import { useState } from "react";
import FallbackCover from "../../../shared/ui/FallbackCover";

const MUSICBRAINZ_ID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function coverSources(group: string, edition: string | null) {
  return [
    ...(edition !== null && MUSICBRAINZ_ID.test(edition)
      ? [`/api/v1/cover-art/release/${edition}`]
      : []),
    ...(MUSICBRAINZ_ID.test(group)
      ? [`/api/v1/cover-art/release-group/${group}`]
      : []),
  ];
}

export default function CatalogCover({
  group,
  edition,
  title,
  artist,
}: {
  group: string;
  edition: string | null;
  title: string;
  artist: string | null;
}) {
  const sources = coverSources(group, edition);
  const key = sources.join("|");
  const [failed, setFailed] = useState<{ key: string; count: number }>({
    key,
    count: 0,
  });
  const skipped = failed.key === key ? failed.count : 0;
  const source = sources[skipped];
  const [loaded, setLoaded] = useState<string>();
  return (
    <div
      className={`artwork ${source ? "has-image" : ""}`}
      data-loaded={source !== undefined && loaded === source ? "" : undefined}
      aria-hidden="true"
    >
      {(source === undefined || loaded !== source) && (
        <FallbackCover
          id={group}
          title={title}
          artist={source === undefined ? artist : null}
          inline={source === undefined}
        />
      )}
      {source !== undefined && (
        <img
          key={source}
          src={source}
          alt=""
          loading="lazy"
          onLoad={() => setLoaded(source)}
          onError={() => setFailed({ key, count: skipped + 1 })}
        />
      )}
    </div>
  );
}
