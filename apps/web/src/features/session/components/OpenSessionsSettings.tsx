import type { Schema } from "../../../shared/schema";

import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { useOpenSessions } from "../hooks/useOpenSessions";

function signedIn(session: Schema<"SessionInfo">) {
  return new Date(session.created_at * 1000).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

export default function OpenSessionsSettings({
  account,
  version,
}: {
  account: string;
  version: number;
}) {
  const list = useOpenSessions(account, version);
  const others = list.sessions?.some((session) => !session.current) ?? false;
  return (
    <SettingsCard
      title="Open sessions"
      description="Every browser or device signed in to your account. Revoke one you do not recognise."
      busy={list.busy}
      action={
        <Button
          disabled={list.busy || !others}
          onClick={() => void list.revokeOthers()}
        >
          Sign out other sessions
        </Button>
      }
    >
      {list.error && (
        <p className="form-error" role="alert">
          {list.error}
        </p>
      )}
      {list.error && list.sessions === null && (
        <Button onClick={list.retry}>Try again</Button>
      )}
      {list.sessions?.map((session) => (
        <SettingRow
          key={session.id}
          title={session.current ? "This session" : "Another session"}
          hint={`Signed in ${signedIn(session)}`}
          action={
            session.current ? undefined : (
              <Button
                variant="destructive"
                disabled={list.busy}
                aria-label={`Revoke session signed in ${signedIn(session)}`}
                onClick={() => void list.revoke(session.id)}
              >
                Revoke
              </Button>
            )
          }
        />
      ))}
    </SettingsCard>
  );
}
