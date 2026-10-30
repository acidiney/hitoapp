import type { Schema } from "../../../shared/schema";

import AccountSettings from "../../accounts/components/AccountSettings";
import SharedCorrectionsSettings from "../../artists/components/SharedCorrectionsSettings";

export default function AccountsSection({
  user,
  libraries,
}: Schema<"SettingsPageProps">) {
  return (
    <>
      <AccountSettings
        key={user.id}
        account={user.id}
        library={libraries.find((library) => !library.personal)?.id}
      />
      <SharedCorrectionsSettings
        key={`shared-corrections:${user.id}`}
        account={user.id}
      />
    </>
  );
}
