import { X } from "lucide-react";
import { useConnection, useLostChange } from "../hooks/useConnection";
import {
  dismissLostChange,
  retryConnectionNow,
  type OutageReason,
} from "../connection";
import Button from "./Button";

const details: Record<OutageReason, string> = {
  network: "Check your internet connection and that Hito is running.",
  timeout: "Hito is taking too long to answer.",
  gateway: "Hito is not responding right now.",
};

export default function ConnectionNotice() {
  const connection = useConnection();
  const lostChange = useLostChange();
  return (
    <>
      {!connection.reachable && (
        <div className="notice connection-notice" role="status">
          <span>
            <strong>Cannot reach Hito.</strong> {details[connection.reason]}{" "}
            Your place is kept. Reconnecting automatically.
          </span>
          <Button onClick={retryConnectionNow}>Try now</Button>
        </div>
      )}
      {lostChange && (
        <div className="notice connection-notice" role="alert">
          <span>
            Your last change was not saved. Check it and try again once Hito is
            reachable.
          </span>
          <button
            className="icon-button"
            aria-label="Dismiss message"
            onClick={dismissLostChange}
          >
            <X size={17} />
          </button>
        </div>
      )}
    </>
  );
}
