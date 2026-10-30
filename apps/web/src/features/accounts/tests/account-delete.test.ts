import { createElement } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import AccountSettings from "../components/AccountSettings";
import { accountsApi } from "../api/accountsApi";
import { NoticeProvider } from "../../../shared/ui/Notices";

vi.mock("../api/accountsApi", () => ({
  accountsApi: {
    list: vi.fn(),
    create: vi.fn(),
    setLibraryAccess: vi.fn(),
    remove: vi.fn(),
  },
}));

const admin = { id: "a", username: "admin", admin: true };
const listener = { id: "b", username: "listener", admin: false };
beforeEach(() => {
  vi.clearAllMocks();
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  vi.mocked(accountsApi.list).mockResolvedValue([admin, listener]);
});
const view = () =>
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(AccountSettings, { account: "a", library: "lib" }),
    ),
  );

async function openDeletion() {
  view();
  await screen.findByText("listener");
  fireEvent.click(screen.getByRole("button", { name: "Delete account" }));
  return screen.findByRole("dialog", { name: "Delete account" });
}

test("only accounts other than the signed-in one offer deletion", async () => {
  view();
  await screen.findByText("listener");
  expect(
    screen.getAllByRole("button", { name: "Delete account" }),
  ).toHaveLength(1);
});

test("deleting needs the exact account name and then removes the row", async () => {
  vi.mocked(accountsApi.remove).mockResolvedValue(undefined);
  const dialog = await openDeletion();
  const confirm = within(dialog).getByRole("button", {
    name: "Delete account",
  });
  expect((confirm as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(within(dialog).getByLabelText("Type listener to confirm"), {
    target: { value: "Listener" },
  });
  expect((confirm as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(within(dialog).getByLabelText("Type listener to confirm"), {
    target: { value: "listener" },
  });
  expect((confirm as HTMLButtonElement).disabled).toBe(false);
  fireEvent.click(confirm);
  await waitFor(() =>
    expect(accountsApi.remove).toHaveBeenCalledWith(
      "a",
      listener,
      expect.any(AbortSignal),
    ),
  );
  await waitFor(() => expect(screen.queryByText("listener")).toBeNull());
  expect(screen.getByText("admin")).toBeTruthy();
  await waitFor(() => expect(dialog.hasAttribute("open")).toBe(false));
});

test("a refused deletion stays in the dialog and keeps the account", async () => {
  vi.mocked(accountsApi.remove).mockRejectedValue(
    new Error("This account changed. Reload Settings."),
  );
  const dialog = await openDeletion();
  fireEvent.change(within(dialog).getByLabelText("Type listener to confirm"), {
    target: { value: "listener" },
  });
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Delete account" }),
  );
  const alert = await within(dialog).findByRole("alert");
  expect(alert.textContent).toContain("This account changed");
  expect(dialog.hasAttribute("open")).toBe(true);
  expect(screen.getAllByText("listener").length).toBeGreaterThan(0);
});

test("cancelling the confirmation sends nothing", async () => {
  const dialog = await openDeletion();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(dialog.hasAttribute("open")).toBe(false);
  expect(accountsApi.remove).not.toHaveBeenCalled();
});
