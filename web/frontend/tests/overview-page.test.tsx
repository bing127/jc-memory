import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { App } from "../src/app.js";

const instanceId = "019b8c36-e7b0-7f41-8b92-cc77e129fc2d";
const namespaceId = "00000000-0000-0000-0000-000000000001";

afterEach(() => {
  vi.unstubAllGlobals();
  window.history.replaceState({}, "", "/");
});

test("overview renders only status and stats returned by the live contracts", async () => {
  window.history.replaceState({}, "", `/instances/${instanceId}/overview`);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = String(input);
      if (url === "/api/v1/session") {
        return Response.json({ authenticated: true, subject: "owner" });
      }
      if (url === "/api/v1/instances") {
        return Response.json([
          {
            id: instanceId,
            name: "本地记忆实例",
            baseUrl: "http://127.0.0.1:8787/",
            namespaceId,
            protocolVersion: "v1",
            createdAt: "2026-08-01T00:00:00.000Z",
            updatedAt: "2026-08-01T00:00:00.000Z",
          },
        ]);
      }
      if (url.endsWith("/status")) {
        return Response.json({
          health: { status: "ok" },
          ready: { status: "ready" },
          serverReady: true,
        });
      }
      if (url.endsWith("/memory/stats") && init?.method === "POST") {
        return Response.json({
          api_version: "v1",
          request_id: "overview-stats-001",
          response: {
            response: "stats",
            payload: {
              scope: { namespace_id: namespaceId },
              fragment_count: 4,
              event_count: 5,
            },
          },
        });
      }
      throw new Error(`unexpected request ${url}`);
    }),
  );

  render(<App />);

  expect(await screen.findByRole("heading", { name: "运行总览" })).not.toBeNull();
  expect(await screen.findByText("4")).not.toBeNull();
  expect(screen.getByText("5")).not.toBeNull();
  expect(screen.getByText("overview-stats-001")).not.toBeNull();
  expect(screen.queryByText(/P95|CPU|模型成本|云账单/)).toBeNull();
});
