import type { Schema } from "../../../shared/schema";

import { useArtistRadio } from "../hooks/useArtistRadio";
import { useFeaturedPlayback } from "../hooks/useFeaturedPlayback";
import { useFeaturedShelf } from "../hooks/useFeaturedShelf";
import { useRadioPreview } from "../hooks/useRadioPreview";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import FeaturedShelf from "../components/FeaturedShelf";
import GenreTiles from "../components/GenreTiles";
import ProviderSearch from "../components/ProviderSearch";
import RadioHero from "../components/RadioHero";
import RadioStarts, { RadioNotice } from "../components/RadioStarts";

function mostOwned(groups: Schema<"ArtistIdentityGroup">[]) {
  return groups.reduce<Schema<"ArtistIdentityGroup"> | null>(
    (best, group) =>
      best === null || group.recording_count > best.recording_count
        ? group
        : best,
    null,
  );
}

export default function DiscoverPage({
  user,
  connections,
  service_names,
  genres,
  artists,
}: Schema<"DiscoverPageProps">) {
  const player = usePlayer();
  const radio = useArtistRadio(user.id, player);
  const featured = useFeaturedShelf(user.id);
  const featuredPlayback = useFeaturedPlayback(user.id, player);
  const hero = mostOwned(artists.items);
  const preview = useRadioPreview(user.id, hero);
  const services = Object.fromEntries(
    connections.map((connection) => [
      connection.id,
      service_names[connection.service_id] ?? connection.manifest.name,
    ]),
  );
  return (
    <div className="discovery-page">
      <header className="discover-header">
        <h1>Discover</h1>
      </header>
      {hero && (
        <RadioHero
          artist={hero}
          mostOwned={artists.total <= artists.items.length}
          preview={preview}
          radio={radio}
          onStart={() => void radio.start(hero)}
        />
      )}
      <RadioNotice notice={radio.notice} />
      <GenreTiles genres={genres} />
      <RadioStarts
        artists={artists.items}
        radio={radio}
        onStart={(artist) => void radio.start(artist)}
      />
      <ProviderSearch
        user={user}
        connections={connections}
        service_names={service_names}
      >
        <FeaturedShelf
          items={featured}
          services={services}
          playback={featuredPlayback}
          onPlay={(entry) => void featuredPlayback.play(entry)}
          onOpen={(entry) => void featuredPlayback.open(entry)}
        />
      </ProviderSearch>
    </div>
  );
}
