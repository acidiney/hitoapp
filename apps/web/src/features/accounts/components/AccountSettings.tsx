import { useEffect, useRef, useState } from "react";
import { UserPlus } from "lucide-react";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { useAccounts } from "../hooks/useAccounts";
import AccountDialog from "./AccountDialog";

export default function AccountSettings({
  account,
  library,
}: {
  account: string;
  library?: string;
}) {
  const { users, pending, create, access } = useAccounts(account);
  const [creating, setCreating] = useState(false);
  const opener = useRef<HTMLButtonElement>(null);
  const wasCreating = useRef(false);
  useEffect(() => {
    if (wasCreating.current && !creating)
      opener.current?.focus({ preventScroll: true });
    wasCreating.current = creating;
  }, [creating]);
  return (
    <SettingsCard
      title="Accounts and access"
      description="People who can sign in to this server."
      action={
        <Button
          ref={opener}
          variant="primary"
          onClick={() => setCreating(true)}
        >
          <UserPlus size={16} aria-hidden="true" />
          Create account
        </Button>
      }
    >
      {users.map((user) => (
        <SettingRow
          key={user.id}
          title={user.username}
          hint={user.admin ? "Administrator" : "Member"}
          wide
          action={
            !user.admin && library ? (
              <>
                <Button onClick={() => void access(library, user, true)}>
                  Grant access
                </Button>
                <Button onClick={() => void access(library, user, false)}>
                  Remove access
                </Button>
              </>
            ) : undefined
          }
        />
      ))}
      {creating && (
        <AccountDialog
          pending={pending}
          onCreate={create}
          onClose={() => setCreating(false)}
        />
      )}
    </SettingsCard>
  );
}
