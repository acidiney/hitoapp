import { useId, useRef } from "react";

import type { Schema } from "../../../shared/schema";
import { accountName } from "../../../shared/accountName";
import Avatar from "../../../shared/ui/Avatar";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import { useAvatarPhoto } from "../hooks/useAvatarPhoto";

export default function AvatarSettings({
  user,
  onSaved,
}: {
  user: Schema<"User">;
  onSaved: () => void;
}) {
  const photo = useAvatarPhoto(user.id, onSaved);
  const input = useRef<HTMLInputElement>(null);
  const limits = useId();
  return (
    <SettingRow
      id="account-photo"
      leading={
        <Avatar
          id={user.id}
          name={accountName(user)}
          version={user.avatar_version}
          large
        />
      }
      title={accountName(user)}
      hint={`${user.display_name ? `${user.username} · ` : ""}${user.admin ? "Administrator" : "Member"}`}
      action={
        <>
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
          <Button
            disabled={photo.busy}
            aria-describedby={limits}
            onClick={() => input.current?.click()}
          >
            Change photo
          </Button>
          {user.avatar_version && (
            <Button
              variant="link"
              disabled={photo.busy}
              onClick={() => void photo.remove()}
            >
              Remove photo
            </Button>
          )}
          <span id={limits} className="sr-only">
            JPEG, PNG or WebP up to 2 MiB. It is cropped to a square. Only you
            and administrators can see it.
          </span>
        </>
      }
    >
      {photo.message && <p role="status">{photo.message}</p>}
      {photo.error && (
        <p className="form-error" role="alert">
          {photo.error}
        </p>
      )}
    </SettingRow>
  );
}
