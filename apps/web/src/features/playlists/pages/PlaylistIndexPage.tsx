import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link } from "@inertiajs/react";
import { Link2, Plus } from "lucide-react";

import ActionMenu from "../../../shared/ui/ActionMenu";
import {
  PHONE_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";
import EmptyState from "../../../shared/ui/EmptyState";
import LibraryHeading from "../../../app/LibraryHeading";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import Artwork from "../../artwork/components/Artwork";
import PlaylistCreateDialog from "../components/PlaylistCreateDialog";
import PlaylistImportDialog from "../components/PlaylistImportDialog";

type Panel = "create" | "import" | null;

export default function PlaylistIndexPage({
  user,
  playlists,
  connections,
  service_names,
}: Schema<"PlaylistIndexPageProps">) {
  const [panel, setPanel] = useState<Panel>(() =>
    window.location.hash === "#new-playlist" ? "create" : null,
  );
  const phone = useMediaQuery(PHONE_QUERY);
  const compactActions = (
    <>
      <Button
        className="icon-only"
        aria-haspopup="dialog"
        aria-label="New playlist"
        onClick={() => setPanel("create")}
      >
        <Plus size={18} aria-hidden="true" />
      </Button>
      <ActionMenu
        label="More actions"
        align="end"
        triggerClassName="icon-only"
        items={[
          { label: "Import from a link", onSelect: () => setPanel("import") },
        ]}
      />
    </>
  );
  const actions = (
    <>
      <Button
        variant="primary"
        aria-haspopup="dialog"
        onClick={() => setPanel("create")}
      >
        <Plus size={17} aria-hidden="true" />
        New playlist
      </Button>
      <Button aria-haspopup="dialog" onClick={() => setPanel("import")}>
        <Link2 size={17} aria-hidden="true" />
        Import from a link
      </Button>
    </>
  );
  return (
    <>
      <LibraryHeading
        path="/playlists"
        title="Playlists"
        description={`${playlists.length} ${playlists.length === 1 ? "playlist" : "playlists"}`}
      >
        {playlists.length > 0 && (
          <div className="library-heading-actions">
            {phone ? compactActions : actions}
          </div>
        )}
      </LibraryHeading>
      {playlists.length > 0 ? (
        <div className="playlist-list">
          {playlists.map((playlist) => (
            <Link href={`/playlists/${playlist.id}`} key={playlist.id}>
              <Artwork
                playlistId={playlist.kind === "local" ? playlist.id : undefined}
                fallback={{
                  id: playlist.id,
                  title: playlist.name,
                  artist: "Playlist",
                  inline: false,
                }}
                playlist
              />
              <strong>{playlist.name}</strong>
              <small>
                {playlist.kind === "followed" ? (
                  <span className="badge">Following</span>
                ) : (
                  "Your playlist"
                )}
              </small>
            </Link>
          ))}
        </div>
      ) : (
        <EmptyState
          title="No playlists yet"
          text="Add tracks from your library, or import one you already have."
          libraryLink={false}
        >
          <div className="empty-actions">{actions}</div>
        </EmptyState>
      )}
      {panel === "create" && (
        <PlaylistCreateDialog
          account={user.id}
          playlists={playlists}
          onClose={() => setPanel(null)}
        />
      )}
      {panel === "import" && (
        <PlaylistImportDialog
          account={user.id}
          connections={connections}
          names={service_names}
          onClose={() => setPanel(null)}
        />
      )}
    </>
  );
}
