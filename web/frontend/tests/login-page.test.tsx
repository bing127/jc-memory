import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";

import { App } from "../src/app.js";

afterEach(() => {
  vi.unstubAllGlobals();
  window.history.replaceState({}, "", "/");
});

test("keeps the password out of the URL and enters the console after login", async () => {
  window.history.replaceState({}, "", "/login");
  const requests: Array<{ url: string; init?: RequestInit }> = [];
  const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    requests.push(init ? { url, init } : { url });
    if (url === "/api/v1/session" && init?.method === "POST") {
      return Response.json({ authenticated: true, subject: "owner" });
    }
    if (url === "/api/v1/session") {
      return Response.json({ code: "unauthorized" }, { status: 401 });
    }
    if (url === "/api/v1/instances") {
      return Response.json([]);
    }
    throw new Error(`unexpected request ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  const user = userEvent.setup();
  render(<App />);

  const password = await screen.findByLabelText("管理员密码");
  await user.type(password, "correct-password");
  await user.click(screen.getByRole("button", { name: "进入控制台" }));

  await screen.findByRole("heading", { name: "连接记忆实例" });
  expect(window.location.search).not.toContain("correct-password");
  const loginRequest = requests.find((request) => request.init?.method === "POST");
  expect(loginRequest?.url).toBe("/api/v1/session");
  expect(JSON.parse(String(loginRequest?.init?.body))).toEqual({
    password: "correct-password",
  });
  await waitFor(() => expect(fetchMock).toHaveBeenCalled());
});
