import { useState, type FormEvent } from "react";
import { useAccounts } from "../hooks/useAccounts";

export default function AccountSettings({
  account,
  library,
}: {
  account: string;
  library?: string;
}) {
  const { users, pending, create, access } = useAccounts(account);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!pending && (await create(username, password))) {
      setUsername("");
      setPassword("");
    }
  }
  return (
    <section id="accounts">
      <h2>Accounts</h2>
      <p>Each account has private playlists, likes, and listening history.</p>
      <form className="account-form" onSubmit={submit}>
        <label>
          Username
          <input
            autoComplete="off"
            value={username}
            maxLength={64}
            required
            onChange={(event) => setUsername(event.target.value)}
          />
        </label>
        <label>
          Password
          <input
            type="password"
            autoComplete="new-password"
            minLength={12}
            value={password}
            required
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        <button className="button primary" type="submit" disabled={pending}>
          Create account
        </button>
      </form>
      <ul className="account-list">
        {users.map((user) => (
          <li key={user.id}>
            <span>
              {user.username}
              {user.admin && <small>Administrator</small>}
            </span>
            {!user.admin && library && (
              <div>
                <button
                  className="button"
                  onClick={() => void access(library, user, true)}
                >
                  Grant access
                </button>
                <button
                  className="button"
                  onClick={() => void access(library, user, false)}
                >
                  Remove access
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
