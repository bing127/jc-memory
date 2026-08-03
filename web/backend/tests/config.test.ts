import { describe, expect, test } from "vitest";

import { loadConfig } from "../src/config.js";

const baseEnvironment = {
  JC_WEB_ADMIN_PASSWORD: "correct-password-long-enough",
  JC_WEB_SESSION_SECRET: Buffer.alloc(32, 1).toString("base64"),
  JC_WEB_CREDENTIAL_KEY: Buffer.alloc(32, 2).toString("base64"),
  NODE_ENV: "test",
};

describe("控制面运行策略配置", () => {
  test("加载默认上游超时和四类统一流采样周期", () => {
    const config = loadConfig(baseEnvironment);

    expect(config.upstreamTimeoutMs).toBe(30_000);
    expect(config.streamIntervals).toEqual({
      overview: 15_000,
      metrics: 1_000,
      runtimeList: 10_000,
      runtimeDetail: 5_000,
    });
  });

  test("环境变量可以覆盖上游超时和所有采样周期", () => {
    const config = loadConfig({
      ...baseEnvironment,
      JC_WEB_UPSTREAM_TIMEOUT_MS: "4500",
      JC_WEB_STREAM_OVERVIEW_INTERVAL_MS: "2500",
      JC_WEB_STREAM_METRICS_INTERVAL_MS: "750",
      JC_WEB_STREAM_RUNTIME_LIST_INTERVAL_MS: "3200",
      JC_WEB_STREAM_RUNTIME_DETAIL_INTERVAL_MS: "1800",
    });

    expect(config.upstreamTimeoutMs).toBe(4500);
    expect(config.streamIntervals).toEqual({
      overview: 2500,
      metrics: 750,
      runtimeList: 3200,
      runtimeDetail: 1800,
    });
  });

  test.each([
    ["JC_WEB_UPSTREAM_TIMEOUT_MS", "0"],
    ["JC_WEB_STREAM_METRICS_INTERVAL_MS", "249"],
    ["JC_WEB_STREAM_RUNTIME_DETAIL_INTERVAL_MS", "300001"],
    ["JC_WEB_STREAM_OVERVIEW_INTERVAL_MS", "10.5"],
  ])("拒绝非法运行策略 %s=%s", (name, value) => {
    expect(() => loadConfig({ ...baseEnvironment, [name]: value })).toThrow(name);
  });
});
