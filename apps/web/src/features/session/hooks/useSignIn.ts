import { useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { errorMessage } from "../../../shared/format";
import { sessionApi } from "../api/sessionApi";

export type SignInFlow = ReturnType<typeof useSignIn>;

export function useSignIn(setup: boolean) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await sessionApi.signIn(setup, username, password);
      router.clearHistory();
      window.location.replace("/library");
    } catch (error) {
      setError(errorMessage(error));
      setBusy(false);
    }
  }
  return { username, setUsername, password, setPassword, busy, error, submit };
}
