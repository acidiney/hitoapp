import { tmpdir } from "node:os";
import { join } from "node:path";

export const adminCredentials = {
  username: "admin",
  password: "docker browser test password",
};
export const adminSessionPath = join(tmpdir(), "hito-e2e-admin-session.json");

export const sharedAdminSession = !Object.entries(process.env).some(
  ([name, value]) => /^HITO_.+_E2E$/.test(name) && value === "1",
);
