import { useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { lockApi } from "../api/lockApi";
import { notifySessionChanged } from "../api/session";

export function useUnlock(account: string) {
  const [pin, setPin] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await lockApi.unlock(account, pin);
      notifySessionChanged();
      router.clearHistory();
      window.location.reload();
    } catch (error) {
      if (error instanceof HttpError && error.status === 401) {
        notifySessionChanged();
        window.location.replace("/");
        return;
      }
      setError(errorMessage(error));
      setPin("");
      setBusy(false);
    }
  }
  return { pin, setPin, busy, error, submit };
}
