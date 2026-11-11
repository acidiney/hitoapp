import { usePage } from "@inertiajs/react";
import { Link2 } from "lucide-react";
import { playlistLinkService } from "../model/playlistService";
import PlaylistImportDialog from "./PlaylistImportDialog";

export function PlaylistLinkHint({
  link,
  onOpen,
}: {
  link: string;
  onOpen: () => void;
}) {
  return (
    <div className="search-link-hint" role="status">
      <Link2 size={18} aria-hidden="true" />
      <span>
        <strong>{playlistLinkService(link)} playlist link</strong>
        <small>Follow it or copy it</small>
      </span>
      <button type="button" onClick={onOpen}>
        Open
      </button>
    </div>
  );
}

export default function PlaylistLinkImport({
  link,
  onClose,
}: {
  link: string;
  onClose: () => void;
}) {
  const { user } = usePage<{ user: { id: string } }>().props;
  return (
    <PlaylistImportDialog
      account={user.id}
      initialReference={link}
      onClose={onClose}
    />
  );
}
