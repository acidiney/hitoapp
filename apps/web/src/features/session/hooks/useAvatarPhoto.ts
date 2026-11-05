import { useState } from "react";
import { errorMessage } from "../../../shared/format";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { accountApi } from "../api/accountApi";

export const maximumPhotoBytes = 2 * 1024 * 1024;
const accepted = new Set(["image/jpeg", "image/png", "image/webp"]);

export function useAvatarPhoto(account: string, onSaved: () => void) {
  const scope = useRequestScope(account);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  async function run(
    work: (signal: AbortSignal) => Promise<unknown>,
    done: string,
  ) {
    const request = scope.capture();
    setBusy(true);
    setError("");
    setMessage("");
    try {
      await work(request.signal);
      if (!request.active()) return;
      setMessage(done);
      onSaved();
    } catch (caught) {
      if (request.active()) setError(errorMessage(caught));
    } finally {
      if (request.active()) setBusy(false);
    }
  }

  async function upload(file: File) {
    if (file.size > maximumPhotoBytes) {
      setMessage("");
      setError("Use a picture of 2 MiB or less.");
      return;
    }
    if (file.type !== "" && !accepted.has(file.type)) {
      setMessage("");
      setError("Use a JPEG, PNG or WebP picture.");
      return;
    }
    await run(
      (signal) => accountApi.setAvatar(account, file, signal),
      "Photo saved.",
    );
  }

  async function remove() {
    await run(
      (signal) => accountApi.removeAvatar(account, signal),
      "Photo removed. Your initials are shown.",
    );
  }

  return { busy, error, message, upload, remove };
}
