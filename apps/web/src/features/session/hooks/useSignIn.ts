import { useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { sessionApi } from "../api/sessionApi";

export type SignInFlow = ReturnType<typeof useSignIn>;

export function useSignIn(needsSetup: boolean) {
  const [setup, setSetup] = useState(needsSetup);
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
      if (setup && error instanceof HttpError && error.status === 409) {
        setSetup(false);
        setError(
          "Someone else finished setting up this server. Sign in with your account.",
        );
      } else {
        setError(errorMessage(error));
      }
      setBusy(false);
    }
  }
  return {
    setup,
    username,
    setUsername,
    password,
    setPassword,
    busy,
    error,
    submit,
  };
}
