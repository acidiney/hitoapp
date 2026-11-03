type Kind = "release" | "release-group" | "recording";

export type MusicBrainzId = { label: string; kind: Kind; id: string | null };

export default function MusicBrainzIds({ ids }: { ids: MusicBrainzId[] }) {
  const held = ids.filter((item) => item.id !== null);
  if (held.length === 0) return null;
  return (
    <section className="musicbrainz-ids" aria-label="MusicBrainz identifiers">
      <h2>MusicBrainz</h2>
      <dl>
        {held.map((item) => (
          <div key={item.kind}>
            <dt>{item.label}</dt>
            <dd>
              <a
                href={`https://musicbrainz.org/${item.kind}/${encodeURIComponent(item.id!)}`}
                target="_blank"
                rel="noreferrer noopener"
              >
                {item.id}
              </a>
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
