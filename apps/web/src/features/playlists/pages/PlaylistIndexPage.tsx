import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link } from "@inertiajs/react";
import { Link2, Plus } from "lucide-react";

import EmptyState from "../../../shared/ui/EmptyState";
import PageHeading from "../../../shared/ui/PageHeading";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import Artwork from "../../artwork/components/Artwork";
import PlaylistCreateDialog from "../components/PlaylistCreateDialog";
import PlaylistImport from "../components/PlaylistImport";

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
      <PageHeading
        title="Playlists"
        description={`${playlists.length} ${playlists.length === 1 ? "playlist" : "playlists"}`}
      >
        {playlists.length > 0 && (
          <div className="page-heading-actions">{actions}</div>
        )}
      </PageHeading>
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
          text="Make a playlist and add tracks from your library, or bring one over from a link."
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
        <Dialog title="Import a playlist" wide onClose={() => setPanel(null)}>
          <PlaylistImport
            key={user.id}
            account={user.id}
            connections={connections}
            names={service_names}
          />
          <div className="dialog-actions">
            <Button onClick={() => setPanel(null)}>Close</Button>
          </div>
        </Dialog>
      )}
    </>
  );
}
