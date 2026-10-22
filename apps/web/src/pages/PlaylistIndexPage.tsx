import { createUuid } from "../shared/uuid";
import { useState, type FormEvent } from "react";
import { Link, router } from "@inertiajs/react";
import { Plus } from "lucide-react";
import type { PlaylistIndexPageProps } from "../shared/types";
import { useMutation } from "../shared/hooks/useMutation";
import EmptyState from "../shared/ui/EmptyState";
import PageHeading from "../shared/ui/PageHeading";
import Artwork from "../components/Artwork";
import PlaylistImport from "../components/playlists/PlaylistImport";

export default function PlaylistIndexPage({
  user,
  playlists,
  connections,
  service_names,
}: PlaylistIndexPageProps) {
  const [name, setName] = useState("");
  const { mutate, pending } = useMutation(user.id);
  async function create(event: FormEvent) {
    event.preventDefault();
    if (!name.trim() || pending) return;
    const result = await mutate({
      kind: "playlist_create",
      playlist_id: createUuid(),
      name,
    });
    if (result.ok) {
      setName("");
      router.reload();
    }
  }
  return (
    <>
      <PageHeading
        title="Playlists"
        description="Make room for the music you come back to."
      />
      <form className="inline-form" id="new-playlist" onSubmit={create}>
        <label htmlFor="playlist-name">New playlist</label>
        <input
          id="playlist-name"
          value={name}
          maxLength={256}
          placeholder="Give it a name"
          onChange={(event) => setName(event.target.value)}
          required
        />
        <button className="button primary" type="submit" disabled={pending}>
          <Plus size={17} />
          Create playlist
        </button>
      </form>
      <PlaylistImport
        key={user.id}
        account={user.id}
        connections={connections}
        names={service_names}
      />
      <div className="playlist-list">
        {playlists.map((playlist) => (
          <Link href={`/playlists/${playlist.id}`} key={playlist.id}>
            <Artwork
              playlistId={playlist.kind === "local" ? playlist.id : undefined}
              playlist
            />
            <span>
              <strong>{playlist.name}</strong>
              <small>
                {playlist.kind === "followed"
                  ? "Followed playlist"
                  : "Your playlist"}
              </small>
            </span>
            <span aria-hidden="true">→</span>
          </Link>
        ))}
      </div>
      {playlists.length === 0 && (
        <EmptyState
          title="A home for your favorites"
          text="Create a playlist, then add tracks from your library."
        />
      )}
    </>
  );
}
