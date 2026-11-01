import type { Schema } from "../../../shared/schema";

import { useArtistRadio } from "../hooks/useArtistRadio";
import { useFeaturedPlayback } from "../hooks/useFeaturedPlayback";
import { useFeaturedShelf } from "../hooks/useFeaturedShelf";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import FeaturedShelf from "../components/FeaturedShelf";
import GenreTiles from "../components/GenreTiles";
import ProviderSearch from "../components/ProviderSearch";
import RadioStarts from "../components/RadioStarts";
import PageHeading from "../../../shared/ui/PageHeading";

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
  return (
    <div className="discovery-page">
      <PageHeading
        title="Discover"
        description="Start from what you already own or what your services feature. Playing a featured card adds a copy to your Playlists; other music from your services stays outside your library until you add it."
      />
      <FeaturedShelf
        items={featured}
        playback={featuredPlayback}
        onPlay={(entry) => void featuredPlayback.play(entry)}
        onOpen={(entry) => void featuredPlayback.open(entry)}
      />
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
      />
    </div>
  );
}
