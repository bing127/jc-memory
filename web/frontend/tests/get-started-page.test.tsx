import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";

import { App } from "../src/app.js";

afterEach(() => {
  vi.unstubAllGlobals();
  window.history.replaceState({}, "", "/");
});

test("rejects an instance API Key outside the branded format before submission", async () => {
  window.history.replaceState({}, "", "/instances");
  const fetchMock = vi.fn(async (input: string | URL | Request) => {
    const url = String(input);
    if (url === "/api/v1/session") {
      return Response.json({ authenticated: true, subject: "owner" });
    }
    if (url === "/api/v1/instances") {
      return Response.json([]);
    }
    throw new Error(`unexpected request ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  const user = userEvent.setup();
  render(<App />);

  await screen.findByRole("heading", { name: "连接记忆实例" });
  await user.type(screen.getByLabelText("实例名称"), "Local Memory");
  await user.type(screen.getByLabelText("Namespace ID"), "00000000-0000-7000-8000-000000000003");
  await user.type(screen.getByLabelText("API Key"), "server-side-instance-key");
  await user.click(screen.getByRole("button", { name: "保存并检查连接" }));

  expect(await screen.findByText("API Key 必须以 sk-jcmemory- 开头，并包含 34 位小写字母或数字")).not.toBeNull();
  expect(fetchMock).toHaveBeenCalledTimes(2);
});
