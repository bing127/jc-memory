import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { MemoryRouter, useLocation } from "react-router-dom";

import { RuntimePage } from "../src/pages/runtime-page.js";

const instance = {
  id: "00000000-0000-0000-0000-000000000010",
  name: "本地实例",
  baseUrl: "http://127.0.0.1:8787",
  namespaceId: "00000000-0000-0000-0000-000000000001",
  protocolVersion: "v1" as const,
  createdAt: "2026-08-01T00:00:00.000Z",
  updatedAt: "2026-08-01T00:00:00.000Z",
};

afterEach(() => vi.unstubAllGlobals());

test("renders persisted runs and opens the real transition timeline", async () => {
  const runId = "00000000-0000-7000-8000-000000000701";
  vi.stubGlobal("fetch", vi.fn(async (input: string | URL | Request) => {
    const url = String(input);
    if (url.endsWith("/runtime/runs?limit=25&offset=0")) {
      return Response.json({
        items: [{
          run_id: runId,
          scope: { namespace_id: instance.namespaceId, tenant_id: null, user_id: null, agent_id: null, project_id: null },
          state: "completed",
          next_step: 4,
          version: 4,
          created_at_us: 1785556800000000,
          updated_at_us: 1785556801000000,
          control: null,
        }],
        total: 1,
        limit: 25,
        offset: 0,
      });
    }
    return Response.json({
      run: {
        run_id: runId,
        scope: { namespace_id: instance.namespaceId, tenant_id: null, user_id: null, agent_id: null, project_id: null },
        state: "completed",
        next_step: 4,
        version: 4,
        created_at_us: 1785556800000000,
        updated_at_us: 1785556801000000,
        control: { state: "paused", resume_state: "reasoning", user_input: null, next_attempt_at_us: null, attempt: 2, updated_at_us: 1785556800500000 },
      },
      transitions: [{
        id: "transition-1",
        run_id: runId,
        step: 0,
        trace_id: "00000000-0000-7000-8000-000000000702",
        from_state: "observing",
        to_state: "recalling",
        input_refs: [],
        output_refs: [],
        attempt: 1,
        deadline_at_us: 1785556860000000,
        payload: { phase: "observe" },
        created_at_us: 1785556800100000,
      }],
    });
  }));
  const user = userEvent.setup();
  renderRuntime();

  expect(await screen.findByText(runId)).not.toBeNull();
  expect(screen.getByText("completed")).not.toBeNull();
  await user.click(screen.getByRole("button", { name: "查看 Run" }));
  expect(screen.getByTestId("runtime-location").textContent).toBe(`?run=${runId}`);
  expect(await screen.findByText("observing -> recalling")).not.toBeNull();
  expect(screen.getAllByText("paused").length).toBeGreaterThan(0);
  expect(screen.getAllByText(/"phase": "observe"/).length).toBeGreaterThan(0);
  expect(screen.queryByText("Approval approved")).toBeNull();
  expect(screen.queryByText("Tool executed")).toBeNull();
});

test("shows the Core boundary when the connected server has no Runtime query API", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => Response.json(
    { code: "unavailable", message: "runtime query service is unavailable" },
    { status: 503 },
  )));
  renderRuntime();

  expect(await screen.findByText("当前实例未开放 Runtime 查询契约")).not.toBeNull();
  expect(screen.queryByRole("button", { name: /启动 Run|批准|执行 Tool/ })).toBeNull();
});

test("starts a real FinalResponse run and selects the returned run id", async () => {
  const runId = "00000000-0000-7000-8000-000000000711";
  const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (init?.method === "POST") {
      expect(JSON.parse(String(init.body))).toEqual({ prompt: "执行真实 Runtime" });
      return Response.json({ outcome: "completed", run_id: runId, state: "completed", response: { text: "完成" }, approval_id: null }, { status: 201 });
    }
    if (url.includes("/runtime/runs?")) return Response.json({ items: [], total: 0, limit: 25, offset: 0 });
    return Response.json({ run: { run_id: runId, state: "completed" }, transitions: [] });
  });
  vi.stubGlobal("fetch", fetchMock);
  const user = userEvent.setup();
  renderRuntime();
  await user.type(await screen.findByLabelText("Runtime Prompt"), "执行真实 Runtime");
  await user.click(screen.getByRole("button", { name: "启动 Run" }));
  expect(await screen.findByText("完成")).not.toBeNull();
  expect(screen.getByTestId("runtime-location").textContent).toContain(`run=${runId}`);
});

function renderRuntime() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`/instances/${instance.id}/runtime`]}>
        <RuntimePage instance={instance} />
        <LocationProbe />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function LocationProbe() {
  const location = useLocation();
  return <output data-testid="runtime-location">{location.search}</output>;
}
