import type { Schema } from "../../../shared/schema";

import { useArtistRadio } from "../hooks/useArtistRadio";
import { usePlayer } from "../../playback/components/PlaybackProvider";
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
  const radio = useArtistRadio(user.id, usePlayer());
  return (
    <div className="discovery-page">
      <PageHeading
        title="Discover"
        description="Start from what you already own. Music from your connected services stays outside your library until you add it."
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
