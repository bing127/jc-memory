import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";

import { MemoryPacket } from "../src/components/memory-packet.js";
import { MemoryLabPage } from "../src/pages/memory-lab-page.js";

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

test("empty packet fields state that the current call path did not provide data", () => {
  render(
    <MemoryPacket
      packet={{
        query_id: "00000000-0000-0000-0000-000000000301",
        intent: "exact",
        current_states: [],
        historical_states: [],
        activated_surfaces: [],
        episodes: [],
        points: [],
        paths: [],
        procedures: [],
        constraints: [],
        preferences: [],
        past_failures: [],
        evidence: [],
        conflicts: [],
        missing_evidence: [],
        coverage: {
          seed_count: 0,
          visited_nodes: 0,
          max_depth: 0,
          evidence_count: 0,
          missing_evidence_count: 0,
          truncated: false,
        },
        confidence: 0,
        recommended_context: "{}",
      }}
    />,
  );

  expect(screen.getAllByText("当前调用链未提供").length).toBeGreaterThan(0);
  expect(screen.queryByText("没有此类记忆")).toBeNull();
  expect(screen.getByText("truncated: false")).not.toBeNull();
  expect(screen.getByText("00000000-0000-0000-0000-000000000301")).not.toBeNull();
});

test("exposes the three stable write operations without calling them automatically", () => {
  renderMemoryLab();

  expect(screen.getByRole("tab", { name: "Commit" })).not.toBeNull();
  expect(screen.getByRole("tab", { name: "Correct" })).not.toBeNull();
  expect(screen.getByRole("tab", { name: "Forget" })).not.toBeNull();
  expect(screen.getByText(/原始 Fragment、Event 和 EvidenceRef 不会被覆盖/)).not.toBeNull();
});

test("Observe defaults to a recallable unverified Fact", async () => {
  const fetchMock = vi.fn(async (_input: string | URL | Request, _init?: RequestInit) => Response.json({
    api_version: "v1",
    request_id: "00000000-0000-0000-0000-000000000901",
    response: { response: "observe", payload: { created: true } },
  }));
  vi.stubGlobal("fetch", fetchMock);
  const user = userEvent.setup();
  renderMemoryLab();

  expect((screen.getByLabelText("Fragment Kind") as HTMLSelectElement).value).toBe("fact");
  await user.type(screen.getByLabelText("原始文本"), "可被立即召回的事实");
  await user.click(screen.getByRole("button", { name: "Observe Fragment" }));

  await screen.findByText("observe", { exact: true });
  const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
  const body = JSON.parse(String(request.body));
  expect(body.fragment.kind).toBe("fact");
  expect(body.fragment.validity).toBe("unverified");
});

test("Forget requires typing the exact target Fragment ID before submission", async () => {
  const user = userEvent.setup();
  renderMemoryLab();
  await user.click(screen.getByRole("tab", { name: "Forget" }));

  const targetId = "00000000-0000-0000-0000-000000000301";
  await user.type(screen.getByLabelText("目标 Fragment ID"), targetId);
  await user.type(screen.getByLabelText("失效原因"), "用户明确要求该记录失效");
  const submit = screen.getByRole("button", { name: "确认追加失效记录" }) as HTMLButtonElement;
  expect(submit.disabled).toBe(true);

  await user.type(screen.getByLabelText("再次输入目标 Fragment ID"), "not-the-target");
  expect(submit.disabled).toBe(true);
  await user.clear(screen.getByLabelText("再次输入目标 Fragment ID"));
  await user.type(screen.getByLabelText("再次输入目标 Fragment ID"), targetId);
  expect(submit.disabled).toBe(false);
  expect(screen.getByText(/Forget 只追加 Invalidated 记录，不执行 Hard Delete/)).not.toBeNull();
});

test("Correct collects the target, replacement content, reason and verification method", async () => {
  const user = userEvent.setup();
  renderMemoryLab();
  await user.click(screen.getByRole("tab", { name: "Correct" }));

  expect(screen.getByLabelText("目标 Fragment ID").hasAttribute("required")).toBe(true);
  expect(screen.getByLabelText("修正内容").hasAttribute("required")).toBe(true);
  expect(screen.getByLabelText("修正原因").hasAttribute("required")).toBe(true);
  expect(screen.getByLabelText("验证方法")).not.toBeNull();
});

function renderMemoryLab() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryLabPage instance={instance} />
    </QueryClientProvider>,
  );
}
