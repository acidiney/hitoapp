import { useEffect } from "react";
import Button from "../shared/ui/Button";
import { retryConnectionNow, watchServer } from "../shared/connection";

export default function ConnectionErrorPage() {
  useEffect(() => watchServer(() => window.location.reload()), []);
  return (
    <main className="session-error">
      <h1>Cannot reach Hito</h1>
      <p>
        Check your internet connection and that Hito is running. This page
        reconnects automatically as soon as it can.
      </p>
      <Button variant="primary" onClick={retryConnectionNow}>
        Try now
      </Button>
    </main>
  );
}
