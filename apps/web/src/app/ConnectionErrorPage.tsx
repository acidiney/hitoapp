import { useEffect } from "react";
import Button from "../shared/ui/Button";
import { watchServer } from "../shared/connection";

const reloadKey = "hito-connection-reload";
const reloadSpacingMs = 15_000;

function reloadOnce() {
  const now = Date.now();
  let last = 0;
  try {
    last = Number(sessionStorage.getItem(reloadKey)) || 0;
  } catch {}
  if (now - last < reloadSpacingMs) return;
  try {
    sessionStorage.setItem(reloadKey, String(now));
  } catch {}
  window.location.reload();
}

export default function ConnectionErrorPage() {
  useEffect(() => watchServer(reloadOnce), []);
  return (
    <main className="session-error">
      <h1>Cannot reach Hito</h1>
      <p>
        Check your internet connection and that Hito is running. This page
        reconnects automatically as soon as it can.
      </p>
      <Button variant="primary" onClick={() => window.location.reload()}>
        Try now
      </Button>
    </main>
  );
}
