import { createElement } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
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

const NOW = Date.UTC(2026, 10, 4, 12, 0, 0);

beforeEach(() => {
  vi.clearAllMocks();
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(() => vi.useRealTimers());

const view = () =>
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(AccountSettings, { account: "a", library: "lib" }),
    ),
  );

test("each account row shows its last active time or that no session is open", async () => {
  vi.mocked(accountsApi.list).mockResolvedValue([
    {
      id: "a",
      username: "admin",
      admin: true,
      last_active_at: NOW / 1000 - 300,
    },
    { id: "b", username: "listener", admin: false, last_active_at: null },
  ]);
  view();
  await screen.findByText("listener");
  expect(
    screen.getByText("Administrator · Last active 5 minutes ago"),
  ).toBeTruthy();
  expect(screen.getByText("Member · No open sessions")).toBeTruthy();
});

test("a row shows the display name with the username beside the role, and falls back to the username", async () => {
  vi.mocked(accountsApi.list).mockResolvedValue([
    {
      id: "a",
      username: "admin",
      admin: true,
      display_name: "Ana Souza",
      last_active_at: NOW / 1000 - 300,
    },
    {
      id: "b",
      username: "listener",
      admin: false,
      display_name: '<img src=x onerror="window.pwned=1">',
      last_active_at: null,
    },
    { id: "c", username: "plain", admin: false, last_active_at: null },
  ]);
  const { container } = view();
  await screen.findByText("Ana Souza");
  expect(
    screen.getByText("admin · Administrator · Last active 5 minutes ago"),
  ).toBeTruthy();
  expect(screen.getByText("listener · Member · No open sessions")).toBeTruthy();
  expect(screen.getByText('<img src=x onerror="window.pwned=1">')).toBeTruthy();
  expect(container.querySelector("img")).toBeNull();
  expect(screen.getByText("plain")).toBeTruthy();
  expect(screen.getByText("Member · No open sessions")).toBeTruthy();
  expect(container.querySelectorAll(".avatar")[0].textContent).toBe("AS");
});

test("a newly created account reads as having no open sessions", async () => {
  vi.mocked(accountsApi.list).mockResolvedValue([
    { id: "a", username: "admin", admin: true, last_active_at: NOW / 1000 },
  ]);
  vi.mocked(accountsApi.create).mockResolvedValue({
    id: "c",
    username: "fresh",
    admin: false,
  });
  view();
  await screen.findByText("Administrator · Last active just now");
  fireEvent.click(screen.getByRole("button", { name: "Create account" }));
  const dialog = await screen.findByRole("dialog", { name: "Create account" });
  fireEvent.change(within(dialog).getByLabelText("Username"), {
    target: { value: "fresh" },
  });
  fireEvent.change(within(dialog).getByLabelText("Password"), {
    target: { value: "a fresh long password" },
  });
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Create account" }),
  );
  await screen.findByText("Member · No open sessions");
});
