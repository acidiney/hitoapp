import type {
  Action,
  Credentials,
  Job,
  Source,
  SyncBatch,
  SyncReceipt,
  User,
} from "./types";
import { request } from "./http";
import { log } from "./log";
import { notifySessionChanged } from "./session";

const part = encodeURIComponent;

export const sessionApi = {
  async signIn(
    setup: boolean,
    username: string,
    password: string,
  ): Promise<void> {
    const action = setup ? "session.setup" : "session.login";
    log.info("session.started", { action });
    try {
      await request(setup ? "/setup" : "/login", {
        action,
        method: "POST",
        body: { username, password } satisfies Credentials,
      });
      notifySessionChanged();
      log.info("session.accepted", { action });
    } catch (error) {
      log.warn("session.failed", { action });
      throw error;
    }
  },
  async signOut(account: string): Promise<void> {
    await request("/logout", {
      action: "session.logout",
      method: "POST",
      account,
    });
    notifySessionChanged();
    log.info("session.ended", { account_id: account });
  },
};

export const libraryApi = {
  async sources(
    account: string,
    recording: string,
    signal?: AbortSignal,
  ): Promise<Source[]> {
    return (
      await request(`/recordings/${part(recording)}/sources`, {
        action: "sources.list",
        account,
        signal,
      })
    ).json();
  },
  async scan(
    account: string,
    library: string,
    signal?: AbortSignal,
  ): Promise<Job> {
    return (
      await request(`/libraries/${part(library)}/scan`, {
        action: "scan.enqueue",
        method: "POST",
        account,
        signal,
      })
    ).json();
  },
  async job(account: string, job: string, signal?: AbortSignal): Promise<Job> {
    return (
      await request(`/jobs/${part(job)}`, {
        action: "job.status",
        account,
        signal,
      })
    ).json();
  },
};

export const accountsApi = {
  async list(account: string, signal?: AbortSignal): Promise<User[]> {
    return (
      await request("/users", { action: "accounts.list", account, signal })
    ).json();
  },
  async create(
    account: string,
    username: string,
    password: string,
    signal?: AbortSignal,
  ): Promise<User> {
    const user: User = await (
      await request("/users", {
        action: "accounts.create",
        method: "POST",
        account,
        body: { username, password } satisfies Credentials,
        signal,
      })
    ).json();
    log.info("account.created", { account_id: user.id });
    return user;
  },
  async setLibraryAccess(
    account: string,
    library: string,
    user: string,
    allowed: boolean,
    signal?: AbortSignal,
  ): Promise<void> {
    await request(`/libraries/${part(library)}/members/${part(user)}`, {
      action: "accounts.library_access",
      method: allowed ? "PUT" : "DELETE",
      account,
      signal,
    });
    log.info("account.access_changed", { account_id: user, enabled: allowed });
  },
};

function deviceId() {
  const saved = localStorage.getItem("hito-device");
  if (saved) return saved;
  const device = crypto.randomUUID();
  localStorage.setItem("hito-device", device);
  return device;
}

export async function change(
  account: string,
  action: Action,
  signal?: AbortSignal,
): Promise<SyncReceipt> {
  const batch: SyncBatch = {
    device_id: deviceId(),
    operations: [{ id: crypto.randomUUID(), action }],
  };
  const operation_id = batch.operations[0].id;
  log.debug("mutation.started", {
    account_id: account,
    operation_id,
    action: action.kind,
  });
  try {
    const receipt: SyncReceipt = await (
      await request("/sync", {
        action: action.kind === "progress_set" ? "progress.save" : "sync.push",
        method: "POST",
        body: batch,
        account,
        signal,
      })
    ).json();
    const accepted = action.kind === "progress_set" ? log.debug : log.info;
    accepted("mutation.accepted", {
      account_id: account,
      operation_id,
      action: action.kind,
    });
    return receipt;
  } catch (error) {
    if (!signal?.aborted)
      log.warn("mutation.failed", {
        account_id: account,
        operation_id,
        action: action.kind,
      });
    throw error;
  }
}
