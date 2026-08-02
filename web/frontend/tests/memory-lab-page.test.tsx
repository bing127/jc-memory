import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";

import { MemoryPacket, type StateViewData } from "../src/components/memory-packet.js";
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

test("Observe State mode sends structured state metadata without changing the JSON value", async () => {
  const fetchMock = vi.fn(async (_input: string | URL | Request, _init?: RequestInit) => Response.json({
    api_version: "v1",
    request_id: "00000000-0000-0000-0000-000000000902",
    response: { response: "observe", payload: { created: true } },
  }));
  vi.stubGlobal("fetch", fetchMock);
  const user = userEvent.setup();
  renderMemoryLab();

  await user.selectOptions(screen.getByLabelText("Fragment Kind"), "constraint");
  await user.click(screen.getByRole("button", { name: "State" }));
  expect((screen.getByLabelText("State key") as HTMLInputElement).value).toBe("project.deployment_strategy");
  expect((screen.getByLabelText("JSON value") as HTMLTextAreaElement).value).toBe('"blue_green"');
  expect((screen.getByLabelText("Fragment Kind") as HTMLSelectElement).value).toBe("fact");
  expect(Array.from((screen.getByLabelText("Fragment Kind") as HTMLSelectElement).options, (option) => option.value)).toEqual([
    "fact",
    "state_transition",
    "correction",
  ]);
  await user.selectOptions(screen.getByLabelText("Fragment Kind"), "state_transition");
  await user.click(screen.getByRole("button", { name: "Text" }));
  expect((screen.getByLabelText("Fragment Kind") as HTMLSelectElement).value).toBe("fact");
  await user.click(screen.getByRole("button", { name: "State" }));
  fireEvent.change(screen.getByLabelText("JSON value"), { target: { value: '{"strategy":"canary","percentage":10}' } });
  await user.type(screen.getByLabelText("Reason"), "降低全量发布风险");
  await user.click(screen.getByRole("button", { name: "Observe State" }));

  await screen.findByText("observe", { exact: true });
  const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
  const body = JSON.parse(String(request.body));
  expect(body.fragment.metadata.state).toEqual({
    key: "project.deployment_strategy",
    value: { strategy: "canary", percentage: 10 },
    reason: "降低全量发布风险",
  });
  expect(body.fragment.kind).toBe("fact");
  expect(body.fragment.validity).toBe("unverified");
});

test("Observe State mode reports invalid JSON and does not send a request", async () => {
  const fetchMock = vi.fn();
  vi.stubGlobal("fetch", fetchMock);
  const user = userEvent.setup();
  renderMemoryLab();

  await user.click(screen.getByRole("button", { name: "State" }));
  fireEvent.change(screen.getByLabelText("JSON value"), { target: { value: "{invalid" } });
  await user.type(screen.getByLabelText("Reason"), "必须保留错误输入");
  await user.click(screen.getByRole("button", { name: "Observe State" }));

  expect(await screen.findByText("请输入有效的 JSON value")).not.toBeNull();
  expect(fetchMock).not.toHaveBeenCalled();
});

test("Correct can optionally carry a State declaration", async () => {
  const fetchMock = vi.fn(async (_input: string | URL | Request, _init?: RequestInit) => Response.json({
    api_version: "v1",
    request_id: "00000000-0000-0000-0000-000000000904",
    response: { response: "mutation", payload: { action: "correct" } },
  }));
  vi.stubGlobal("fetch", fetchMock);
  const user = userEvent.setup();
  renderMemoryLab();
  await user.click(screen.getByRole("tab", { name: "Correct" }));
  await user.type(screen.getByLabelText("目标 Fragment ID"), "00000000-0000-0000-0000-000000000301");
  await user.type(screen.getByLabelText("修正内容"), "部署策略修正为 canary");
  await user.type(screen.getByLabelText("修正原因"), "人工确认策略变更");
  await user.click(screen.getByRole("checkbox", { name: "附带 State declaration" }));
  await user.type(screen.getByLabelText("State reason"), "降低全量发布风险");
  await user.clear(screen.getByLabelText("State JSON value"));
  await user.type(screen.getByLabelText("State JSON value"), '"canary"');
  await user.click(screen.getByRole("button", { name: "提交修正" }));

  await screen.findByText("mutation", { exact: true });
  const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
  const body = JSON.parse(String(request.body));
  expect(body.state).toEqual({
    key: "project.deployment_strategy",
    value: "canary",
    reason: "降低全量发布风险",
  });
});

test.each(["current_state", "historical_state"])(
  "%s uses State key as query text and hides entities",
  async (mode) => {
    const fetchMock = vi.fn(async (_input: string | URL | Request, _init?: RequestInit) => Response.json({
      api_version: "v1",
      request_id: "00000000-0000-0000-0000-000000000903",
      response: { response: "recall", payload: emptyPacket(mode) },
    }));
    vi.stubGlobal("fetch", fetchMock);
    const user = userEvent.setup();
    renderMemoryLab();
    await user.click(screen.getByRole("tab", { name: "Recall" }));
    await user.selectOptions(screen.getByLabelText("Recall Mode"), mode);

    expect(screen.queryByLabelText("Entities（逗号分隔）")).toBeNull();
    await user.type(screen.getByLabelText("State key"), "project.deployment_strategy");
    await user.click(screen.getByRole("button", { name: "Run Recall" }));

    await screen.findByText("recall", { exact: true });
    const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
    const body = JSON.parse(String(request.body));
    expect(body.mode).toBe(mode);
    expect(body.text).toBe("project.deployment_strategy");
    expect(body.entities).toEqual([]);
  },
);

test("renders current State as a table and historical State as a descending timeline", () => {
  render(
    <MemoryPacket
      packet={{
        ...emptyPacket("current_state"),
        current_states: [stateView("canary", "2026-08-02T00:00:00Z", null, "切换为金丝雀", "502")],
        historical_states: [
          stateView("rolling", "2026-07-15T00:00:00Z", "2026-08-01T00:00:00Z", "短期滚动发布", "501"),
          stateView("blue_green", "2026-07-01T00:00:00Z", "2026-07-15T00:00:00Z", "初始蓝绿发布", "500"),
        ],
      }}
    />,
  );

  expect(screen.getByRole("table", { name: "Current States" })).not.toBeNull();
  expect(screen.getByText("切换为金丝雀")).not.toBeNull();
  expect(screen.getByText("00000000-0000-0000-0000-000000000502")).not.toBeNull();
  const timeline = screen.getByRole("list", { name: "Historical States" });
  expect(timeline.textContent?.indexOf("rolling")).toBeLessThan(timeline.textContent?.indexOf("blue_green") ?? 0);
  expect(screen.getByText("初始蓝绿发布")).not.toBeNull();
});

test("falls back to source Event when State evidence has no displayable identifier", () => {
  const state = stateView("canary", "2026-08-02T00:00:00Z", null, "切换为金丝雀", "502");
  state.evidence = [{
    fragment_id: "00000000-0000-0000-0000-000000000101",
    event_id: null,
    evidence_ref_id: null,
  }];
  state.source_event_id = "00000000-0000-0000-0000-000000000299";

  render(<MemoryPacket packet={{ ...emptyPacket("current_state"), current_states: [state] }} />);

  expect(screen.getByText("00000000-0000-0000-0000-000000000299")).not.toBeNull();
});

function emptyPacket(intent: string) {
  return {
    query_id: "00000000-0000-0000-0000-000000000301",
    intent,
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
    confidence: 1,
    recommended_context: "",
  };
}

function stateView(value: string, validFrom: string, validTo: string | null, reason: string, evidenceSuffix: string): StateViewData {
  return {
    state_id: "00000000-0000-0000-0000-000000000401",
    version_id: `00000000-0000-0000-0000-0000000004${evidenceSuffix.slice(-2)}`,
    key: "project.deployment_strategy",
    value,
    valid_from: validFrom,
    valid_to: validTo,
    validity: "verified",
    evidence: [{
      fragment_id: "00000000-0000-0000-0000-000000000101",
      event_id: "00000000-0000-0000-0000-000000000201",
      evidence_ref_id: `00000000-0000-0000-0000-000000000${evidenceSuffix}`,
    }],
    reason,
    source_event_id: "00000000-0000-0000-0000-000000000201",
  };
}

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
