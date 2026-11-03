import { useState, type FormEvent } from "react";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { accountApi } from "../api/accountApi";

const minimumLength = 12;

function failure(error: unknown) {
  if (error instanceof HttpError && error.code === "wrong_password")
    return "The current password is not correct.";
  if (error instanceof HttpError && error.status === 429)
    return `${error.message} If you do not recognise a session, revoke it under Open sessions below.`;
  return errorMessage(error);
}

export function usePasswordChange(account: string, onChanged: () => void) {
  const scope = useRequestScope(account);
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  async function submit(event: FormEvent) {
    event.preventDefault();
    setMessage("");
    if (next.length < minimumLength) {
      setError(`Use a new password of at least ${minimumLength} characters.`);
      return;
    }
    if (next !== again) {
      setError("The new passwords do not match.");
      return;
    }
    const request = scope.capture();
    setBusy(true);
    setError("");
    try {
      const result = await accountApi.changePassword(
        account,
        { current_password: current, new_password: next },
        request.signal,
      );
      if (!request.active()) return;
      setCurrent("");
      setNext("");
      setAgain("");
      setMessage(
        result.revoked === 0
          ? "Password changed."
          : `Password changed. ${result.revoked} other ${result.revoked === 1 ? "session was" : "sessions were"} signed out.`,
      );
      onChanged();
    } catch (caught) {
      if (request.active()) setError(failure(caught));
    } finally {
      if (request.active()) setBusy(false);
    }
  }

  return {
    current,
    setCurrent,
    next,
    setNext,
    again,
    setAgain,
    busy,
    error,
    message,
    submit,
    dirty: current !== "" || next !== "" || again !== "",
  };
}
