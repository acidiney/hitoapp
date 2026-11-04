import { useEffect } from "react";
import Button from "../shared/ui/Button";
import { getConnection, watchServer } from "../shared/connection";
import { markReload, reloadWait } from "./reloadGuard";

export default function ConnectionErrorPage() {
  useEffect(() => {
    let retry: ReturnType<typeof setTimeout> | undefined;
    function attempt() {
      clearTimeout(retry);
      const wait = reloadWait();
      if (wait > 0) {
        retry = setTimeout(() => {
          if (getConnection().reachable) attempt();
        }, wait);
        return;
      }
      markReload();
      window.location.reload();
    }
    const stop = watchServer(attempt);
    return () => {
      stop();
      clearTimeout(retry);
    };
  }, []);
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
