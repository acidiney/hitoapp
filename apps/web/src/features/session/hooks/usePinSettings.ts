import type { Schema } from "../../../shared/schema";
import { useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { errorMessage } from "../../../shared/format";

import { lockApi } from "../api/lockApi";

export function usePinSettings(account: string, initial: Schema<"LockStatus">) {
  const [status, setStatus] = useState(initial);
  const [pin, setPin] = useState("");
  const [password, setPassword] = useState("");
  const [idleMinutes, setIdleMinutes] = useState(initial.idle_minutes);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  async function run(
    change: () => Promise<Schema<"LockStatus">>,
    saved: string,
  ) {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const next = await change();
      setStatus(next);
      setIdleMinutes(next.idle_minutes);
      setPin("");
      setPassword("");
      router.reload({ only: ["lock"] });
      setMessage(saved);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  function save(event: FormEvent) {
    event.preventDefault();
    void run(
      () =>
        lockApi.setPin(account, {
          password,
          pin: pin === "" ? null : pin,
          idle_minutes: idleMinutes,
        }),
      "Screen lock saved.",
    );
  }

  function remove() {
    void run(() => lockApi.removePin(account, { password }), "PIN removed.");
  }

  return {
    status,
    pin,
    setPin,
    password,
    setPassword,
    idleMinutes,
    setIdleMinutes,
    busy,
    error,
    message,
    save,
    remove,
  };
}
