import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test } from "vitest";

import { PerformancePage } from "../src/pages/performance-page.js";
import { parseLoadTestReport, summarizeLoadTestReport } from "../src/lib/performance.js";

const instance = {
  id: "instance-1",
  name: "本地记忆实例",
  baseUrl: "http://127.0.0.1:8787/",
  namespaceId: "namespace-1",
  protocolVersion: "v1" as const,
  createdAt: "2026-08-01T00:00:00.000Z",
  updatedAt: "2026-08-01T00:00:00.000Z",
};

function report(overrides: Record<string, unknown> = {}) {
  return {
    schema_version: "jcmem.load-test.v1",
    provenance: { git_commit: "abc123", dirty: false, platform: "darwin-arm64" },
    config: {
      target: "http://127.0.0.1:8787",
      warmup_requests: 10,
      timeout_ms: 30000,
      real_rust_sqlite: true,
    },
    scenarios: [
      {
        name: "recall-c32",
        requests: 100,
        concurrency: 32,
        success: 98,
        failure: 2,
        status_counts: { "200": 98, "429": 2 },
        errors: {},
        throughput_rps: 13.48,
        latency_ms: { p50: 2320.82, p95: 2738.33, p99: null, max: 3200 },
        real_rust_sqlite: true,
      },
    ],
    ...overrides,
  };
}

test("导入合法压测报告后显示核心指标和429", async () => {
  const user = userEvent.setup();
  render(<PerformancePage instance={instance} />);

  const input = screen.getByLabelText("导入压测报告");
  const file = new File([JSON.stringify(report())], "load-report.json", {
    type: "application/json",
  });
  await user.upload(input, file);

  expect(await screen.findByText("recall-c32")).not.toBeNull();
  expect(screen.getByText("100")).not.toBeNull();
  expect(screen.getAllByText("98.0%").length).toBeGreaterThan(0);
  expect(screen.getAllByText("13.48 req/s").length).toBeGreaterThan(0);
  expect(screen.getAllByText("2,320.82 ms").length).toBeGreaterThan(0);
  expect(screen.getAllByText("2").length).toBeGreaterThan(0);
  expect(screen.getAllByText("未采集").length).toBeGreaterThan(0);
  expect(screen.getByText("abc123")).not.toBeNull();
});

test("未知报告版本显示中文错误且不渲染报告数据", async () => {
  const user = userEvent.setup();
  render(<PerformancePage instance={instance} />);
  const input = screen.getByLabelText("导入压测报告");
  const file = new File(
    [JSON.stringify({ ...report(), schema_version: "unknown.v2" })],
    "unknown.json",
    { type: "application/json" },
  );
  await user.upload(input, file);

  expect((await screen.findByRole("alert")).textContent).toContain("不支持的压测报告版本");
  expect(screen.queryByText("recall-c32")).toBeNull();
});

test("多场景摘要使用已采集延迟的最高值", () => {
  const first = report().scenarios[0];
  const parsed = parseLoadTestReport(report({
    scenarios: [
      first,
      {
        ...first,
        name: "recall-c64",
        latency_ms: { p50: 4863.74, p95: 5605.15, p99: null, max: 6439.92 },
      },
    ],
  }));

  expect(summarizeLoadTestReport(parsed).latencyMax).toEqual({
    p50: 4863.74,
    p95: 5605.15,
    p99: null,
  });
});

test("报告解析拒绝版本契约之外的字段", () => {
  expect(() => parseLoadTestReport({ ...report(), api_key: "不应进入报告" })).toThrow(
    "压测报告格式不完整或包含无效指标",
  );
});
