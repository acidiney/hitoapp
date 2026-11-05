import { useState, type FormEvent } from "react";
import { errorMessage } from "../../../shared/format";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { accountApi } from "../api/accountApi";

const maximumCharacters = 64;

export function useDisplayName(
  account: string,
  saved: string | null,
  onSaved: () => void,
) {
  const scope = useRequestScope(account);
  const [baseline, setBaseline] = useState(saved ?? "");
  const [value, setValue] = useState(saved ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const trimmed = value.trim();
  const dirty = trimmed !== baseline;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setMessage("");
    if (Array.from(trimmed).length > maximumCharacters) {
      setError(`Use at most ${maximumCharacters} characters.`);
      return;
    }
    const request = scope.capture();
    setBusy(true);
    setError("");
    try {
      const user = await accountApi.setDisplayName(
        account,
        trimmed === "" ? null : trimmed,
        request.signal,
      );
      if (!request.active()) return;
      const next = user.display_name ?? "";
      setBaseline(next);
      setValue(next);
      setMessage(
        next === ""
          ? "Display name removed. Your username is shown."
          : "Display name saved.",
      );
      onSaved();
    } catch (caught) {
      if (request.active()) setError(errorMessage(caught));
    } finally {
      if (request.active()) setBusy(false);
    }
  }

  return { value, setValue, busy, error, message, submit, dirty };
}
