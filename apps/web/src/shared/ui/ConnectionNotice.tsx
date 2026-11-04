import { useConnection } from "../hooks/useConnection";
import { retryConnectionNow, type OutageReason } from "../connection";
import Button from "./Button";

const details: Record<OutageReason, string> = {
  network: "Check your internet connection and that Hito is running.",
  timeout: "Hito is taking too long to answer.",
  gateway: "Hito is not responding right now.",
};

export default function ConnectionNotice() {
  const connection = useConnection();
  if (connection.reachable) return null;
  return (
    <div className="notice connection-notice" role="status">
      <span>
        <strong>Cannot reach Hito.</strong> {details[connection.reason]} Your
        place is kept. Reconnecting automatically.
      </span>
      <Button onClick={retryConnectionNow}>Try now</Button>
    </div>
  );
}
