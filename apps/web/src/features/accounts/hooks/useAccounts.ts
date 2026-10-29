import { useEffect, useState } from "react";
import { accountsApi } from "../api/accountsApi";
import { errorMessage } from "../../../shared/format";
import { useNotice } from "../../../shared/ui/Notices";
import type { User } from "../../../shared/types";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

export function useAccounts(account: string) {
  const notice = useNotice();
  const scope = useRequestScope(account);
  const [listing, setListing] = useState<{
    scope: typeof scope;
    users: User[];
  }>({ scope, users: [] });
  const [pendingScope, setPendingScope] = useState<typeof scope | null>(null);
  useEffect(() => {
    const request = scope.capture();
    void accountsApi
      .list(account, request.signal)
      .then((result) => {
        if (request.active())
          setListing((current) => {
            const known = current.scope === scope ? current.users : [];
            const users = [
              ...new Map(
                [...result, ...known].map((user) => [user.id, user]),
              ).values(),
            ];
            return { scope, users };
          });
      })
      .catch((error) => {
        if (request.active()) notice(errorMessage(error));
      });
  }, [account, notice, scope]);

  async function create(
    username: string,
    password: string,
  ): Promise<string | null> {
    const request = scope.capture();
    setPendingScope(scope);
    try {
      const user = await accountsApi.create(
        account,
        username,
        password,
        request.signal,
      );
      if (!request.active()) return null;
      setListing((current) => ({
        scope,
        users: [...(current.scope === scope ? current.users : []), user],
      }));
      notice(`Created account ${user.username}. Grant library access below.`);
      return null;
    } catch (error) {
      return request.active() ? errorMessage(error) : null;
    } finally {
      if (request.active()) setPendingScope(null);
    }
  }

  async function access(library: string, user: User, allowed: boolean) {
    const request = scope.capture();
    try {
      await accountsApi.setLibraryAccess(
        account,
        library,
        user.id,
        allowed,
        request.signal,
      );
      if (request.active())
        notice(
          `${allowed ? "Granted" : "Removed"} library access for ${user.username}.`,
        );
    } catch (error) {
      if (request.active()) notice(errorMessage(error));
    }
  }
  return {
    users: listing.scope === scope ? listing.users : [],
    pending: pendingScope === scope,
    create,
    access,
  };
}
