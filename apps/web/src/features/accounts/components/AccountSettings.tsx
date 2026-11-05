import { useEffect, useRef, useState } from "react";
import { Trash2, UserPlus } from "lucide-react";
import type { Schema } from "../../../shared/schema";
import { accountName } from "../../../shared/accountName";
import Avatar from "../../../shared/ui/Avatar";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { accountHint } from "../model/accountText";
import { useAccounts } from "../hooks/useAccounts";
import AccountDialog from "./AccountDialog";
import DeleteAccountDialog from "./DeleteAccountDialog";
import { setting } from "../../settings/model/settingsCatalog";

export default function AccountSettings({
  account,
  library,
}: {
  account: string;
  library?: string;
}) {
  const { users, pending, create, remove, access } = useAccounts(account);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<Schema<"User"> | null>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const wasCreating = useRef(false);
  useEffect(() => {
    if (wasCreating.current && !creating)
      opener.current?.focus({ preventScroll: true });
    wasCreating.current = creating;
  }, [creating]);
  return (
    <SettingsCard
      setting={setting("accounts-access")}
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
          title={accountName(user)}
          leading={
            <Avatar
              id={user.id}
              name={accountName(user)}
              version={user.avatar_version}
            />
          }
          hint={accountHint(user)}
          wide
          action={
            <>
              {!user.admin && library && (
                <>
                  <Button onClick={() => void access(library, user, true)}>
                    Grant access
                  </Button>
                  <Button onClick={() => void access(library, user, false)}>
                    Remove access
                  </Button>
                </>
              )}
              {user.id !== account && (
                <Button onClick={() => setDeleting(user)}>
                  <Trash2 size={16} aria-hidden="true" />
                  Delete account
                </Button>
              )}
            </>
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
      {deleting && (
        <DeleteAccountDialog
          user={deleting}
          pending={pending}
          onDelete={remove}
          onClose={() => setDeleting(null)}
        />
      )}
    </SettingsCard>
  );
}
