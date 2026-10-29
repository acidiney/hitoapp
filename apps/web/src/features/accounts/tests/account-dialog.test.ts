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
  accountsApi: { list: vi.fn(), create: vi.fn(), setLibraryAccess: vi.fn() },
}));

const admin = { id: "a", username: "admin", admin: true };
beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  vi.mocked(accountsApi.list).mockResolvedValue([admin]);
});
const view = () =>
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(AccountSettings, { account: "a", library: "lib" }),
    ),
  );

test("creating an account happens in a dialog opened from the list header", async () => {
  vi.mocked(accountsApi.create).mockResolvedValue({
    id: "b",
    username: "listener",
    admin: false,
  });
  view();
  await screen.findByText("admin");
  expect(screen.queryByLabelText("Username")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Create account" }));
  const dialog = await screen.findByRole("dialog", { name: "Create account" });
  fireEvent.change(within(dialog).getByLabelText("Username"), {
    target: { value: "listener" },
  });
  fireEvent.change(within(dialog).getByLabelText("Password"), {
    target: { value: "a long enough passphrase" },
  });
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Create account" }),
  );
  await waitFor(() =>
    expect(accountsApi.create).toHaveBeenCalledWith(
      "a",
      "listener",
      "a long enough passphrase",
      expect.any(AbortSignal),
    ),
  );
  await screen.findByText("listener");
  await waitFor(() => expect(dialog.hasAttribute("open")).toBe(false));
});

test("cancelling the dialog discards the draft without a request", async () => {
  view();
  await screen.findByText("admin");
  fireEvent.click(screen.getByRole("button", { name: "Create account" }));
  const dialog = await screen.findByRole("dialog", { name: "Create account" });
  fireEvent.change(within(dialog).getByLabelText("Username"), {
    target: { value: "draft" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(dialog.hasAttribute("open")).toBe(false);
  expect(accountsApi.create).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Create account" }));
  expect(
    (await screen.findByLabelText<HTMLInputElement>("Username")).value,
  ).toBe("");
});

test("the account list offers no public sign-up or invite link", async () => {
  view();
  await screen.findByText("admin");
  expect(screen.queryByRole("link")).toBeNull();
});
