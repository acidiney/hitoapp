import { useState } from "react";

import OpenSessionsSettings from "./OpenSessionsSettings";
import PasswordChangeSettings from "./PasswordChangeSettings";

export default function AccountSecurity({ account }: { account: string }) {
  const [version, setVersion] = useState(0);
  return (
    <>
      <PasswordChangeSettings
        account={account}
        onChanged={() => setVersion((count) => count + 1)}
      />
      <OpenSessionsSettings account={account} version={version} />
    </>
  );
}
