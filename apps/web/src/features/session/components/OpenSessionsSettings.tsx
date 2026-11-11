import { useState } from "react";

import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { useOpenSessions } from "../hooks/useOpenSessions";
import { sessionClient, sessionDetail, signedIn } from "../sessionText";
import { setting } from "../../settings/model/settingsCatalog";

const shownFirst = 4;

export default function OpenSessionsSettings({
  account,
  version,
}: {
  account: string;
  version: number;
}) {
  const list = useOpenSessions(account, version);
  const [all, setAll] = useState(false);
  const others = list.sessions?.some((session) => !session.current) ?? false;
  const sessions = list.sessions ?? [];
  const hidden = all ? 0 : Math.max(sessions.length - shownFirst, 0);
  return (
    <SettingsCard
      setting={setting("open-sessions")}
      description="Every browser or device signed in to your account."
      busy={list.busy}
      action={
        <Button
          variant="link"
          disabled={list.busy || !others}
          onClick={() => void list.revokeOthers()}
        >
          Sign out everywhere else
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
      {sessions.slice(0, all ? undefined : shownFirst).map((session) => (
        <SettingRow
          key={session.id}
          title={sessionClient(session)}
          hint={sessionDetail(session)}
          action={
            session.current ? (
              <span className="setting-line-note">This device</span>
            ) : (
              <Button
                variant="link"
                disabled={list.busy}
                aria-label={`Sign out ${sessionClient(session)}, signed in ${signedIn(session)}`}
                onClick={() => void list.revoke(session.id)}
              >
                Sign out
              </Button>
            )
          }
        />
      ))}
      {(hidden > 0 || (all && sessions.length > shownFirst)) && (
        <Button
          variant="link"
          className="settings-more"
          aria-expanded={all}
          onClick={() => setAll(!all)}
        >
          {all ? "Show fewer" : `Show ${hidden} older`}
        </Button>
      )}
    </SettingsCard>
  );
}
