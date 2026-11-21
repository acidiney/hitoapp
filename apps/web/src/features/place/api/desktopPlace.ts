import { invoke } from "@tauri-apps/api/core";

export type ConnectOutcome =
  | { outcome: "opened" }
  | { outcome: "not_an_address" }
  | { outcome: "no_answer"; shown: string }
  | { outcome: "not_hito"; host: string }
  | { outcome: "certificate"; host: string }
  | { outcome: "server_too_old"; version: string | null }
  | { outcome: "app_too_old" }
  | { outcome: "not_encrypted"; origin: string; shown: string }
  | { outcome: "busy" }
  | { outcome: "failed" };

export type RecentServer = {
  origin: string;
  shown: string;
  account: string | null;
  used_at: number;
};

export type ServerPlace = {
  connect: (address: string, acceptPlain: boolean) => Promise<ConnectOutcome>;
  cancel: () => Promise<void>;
  recent: () => Promise<RecentServer[]>;
  forget: (origin: string) => Promise<{ told: boolean }>;
  saved: () => Promise<string | null>;
};

export const serverPlace: ServerPlace = {
  connect: (address, acceptPlain) =>
    invoke<ConnectOutcome>("connect_server", { address, acceptPlain }),
  cancel: () => invoke<void>("cancel_connect"),
  recent: () => invoke<RecentServer[]>("recent_servers"),
  forget: (origin) => invoke<{ told: boolean }>("forget_server", { origin }),
  saved: () => invoke<string | null>("saved_server"),
};

export const desktopPlace = {
  startHere: () => invoke<void>("use_this_computer"),
};
