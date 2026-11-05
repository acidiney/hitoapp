import { useRef } from "react";
import { ImagePlus, Trash2 } from "lucide-react";

import Button from "../../../shared/ui/Button";
import { useAvatarPhoto } from "../hooks/useAvatarPhoto";

export default function AvatarSettings({
  account,
  hasPhoto,
  onSaved,
}: {
  account: string;
  hasPhoto: boolean;
  onSaved: () => void;
}) {
  const photo = useAvatarPhoto(account, onSaved);
  const input = useRef<HTMLInputElement>(null);
  return (
    <div className="avatar-settings">
      <input
        ref={input}
        className="sr-only"
        type="file"
        accept="image/jpeg,image/png,image/webp"
        aria-label="Profile photo file"
        tabIndex={-1}
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (file) void photo.upload(file);
        }}
      />
      <div className="avatar-settings-actions">
        <Button disabled={photo.busy} onClick={() => input.current?.click()}>
          <ImagePlus size={16} aria-hidden="true" />
          Upload photo
        </Button>
        {hasPhoto && (
          <Button disabled={photo.busy} onClick={() => void photo.remove()}>
            <Trash2 size={16} aria-hidden="true" />
            Remove photo
          </Button>
        )}
      </div>
      <small>
        JPEG, PNG or WebP up to 2 MiB. It is cropped to a square. Only you and
        administrators can see it.
      </small>
      {photo.message && <p role="status">{photo.message}</p>}
      {photo.error && (
        <p className="form-error" role="alert">
          {photo.error}
        </p>
      )}
    </div>
  );
}
