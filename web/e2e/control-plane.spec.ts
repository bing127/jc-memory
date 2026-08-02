import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import { join } from "node:path";

const namespaceId = "00000000-0000-0000-0000-000000000001";
const apiKey = "sk-jcmemory-87s92kdp5vqz4mnh3cr0xfbj6gwy1aue7t";
const memoryPort = Number(process.env.JC_WEB_E2E_MEMORY_PORT ?? 8787);
const isRealServer = process.env.JC_WEB_E2E_REAL_SERVER === "1";
const performanceFixture = join(process.cwd(), "e2e", "fixtures", "performance-report.json");

test("login, connect and execute the complete Memory Console path through the BFF", async ({ page, request }, testInfo) => {
  test.setTimeout(90_000);
  const apiResponses: string[] = [];
  const browserRequests: Array<{ method: string; url: string }> = [];
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    const urls: string[] = [];
    class CountingWebSocket extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        urls.push(String(url));
      }
    }
    Object.defineProperty(window, "WebSocket", { value: CountingWebSocket });
    Object.defineProperty(window, "__instanceStreamUrls", { value: urls });
  });
  page.on("request", (browserRequest) => {
    browserRequests.push({ method: browserRequest.method(), url: browserRequest.url() });
  });
  page.on("response", async (response) => {
    if (response.url().includes("/api/")) {
      apiResponses.push(await response.text());
    }
  });

  await page.goto("/");
  const memoryUniverse = page.locator("canvas[data-memory-universe]");
  await expect(memoryUniverse).toBeVisible();
  const sampleUniverseSignal = async () => memoryUniverse.evaluate((canvas: HTMLCanvasElement) => new Promise<number>((resolve) => {
    requestAnimationFrame(() => {
      const gl = canvas.getContext("webgl2") ?? canvas.getContext("webgl");
      if (!gl) {
        resolve(0);
        return;
      }
      const pixels = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4);
      gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      let signalPixels = 0;
      for (let index = 0; index < pixels.length; index += 32) {
        const red = pixels[index] ?? 0;
        const green = pixels[index + 1] ?? 0;
        const blue = pixels[index + 2] ?? 0;
        if (green > 35 && green > red * 1.15 && green > blue * 0.8) signalPixels += 1;
      }
      resolve(signalPixels / (pixels.length / 32));
    });
  }));
  await expect.poll(sampleUniverseSignal).toBeGreaterThan(0.008);
  const maximumUniverseSignal = testInfo.project.name.includes("mobile") ? 0.035 : 0.025;
  await expect.poll(sampleUniverseSignal).toBeLessThan(maximumUniverseSignal);
  await page.screenshot({ path: testInfo.outputPath("landing-page.png"), fullPage: true });

  await page.getByRole("link", { name: "Get Started" }).click();
  await page.getByLabel("管理员密码").fill("e2e-control-plane-password");
  await page.getByRole("button", { name: "进入控制台" }).click();

  await expect(page.getByRole("heading", { name: "连接记忆实例" })).toBeVisible();
  await page.getByLabel("实例名称").fill(`Fixture ${testInfo.project.name}`);
  await page.getByLabel("Server URL").fill(`http://127.0.0.1:${memoryPort}`);
  await page.getByLabel("Namespace ID").fill(namespaceId);
  await page.getByLabel("API Key").fill(apiKey);
  await page.getByRole("button", { name: /保存并检查连接/ }).click();

  await expect(page.getByRole("heading", { name: "运行总览" })).toBeVisible();
  await expect(page.getByText("server_ready")).toHaveClass(/status-ok/);
  if (testInfo.project.name.includes("mobile")) {
    await page.getByRole("button", { name: "打开导航" }).click();
  }
  await page.getByRole("link", { name: "Memory Lab" }).click();
  await expect(page.getByRole("heading", { name: "Memory Lab" })).toBeVisible();

  await page.getByRole("button", { name: "State" }).click();
  await page.getByLabel("State key").fill("project.deployment_strategy");
  await page.getByLabel("JSON value").fill('"blue_green"');
  await page.getByLabel("Reason").fill("生产环境先使用蓝绿发布");
  const observeResponse = page.waitForResponse((response) =>
    response.url().includes("/memory/observe") && response.request().method() === "POST",
  );
  await page.getByRole("button", { name: "Observe State" }).click();
  const observeEnvelope = await (await observeResponse).json();
  const blueGreenFragmentId = observeEnvelope.response.payload.fragment_id as string;
  const blueGreenEventId = observeEnvelope.response.payload.event_id as string;
  await expect(page.getByText("observe", { exact: true })).toBeVisible();

  if (!isRealServer) {
    await expectFixtureMutationRejected(request, blueGreenFragmentId, {
      namespaceId: "00000000-0000-0000-0000-000000000099",
      stateKey: "project.deployment_strategy",
    });
    await expectFixtureMutationRejected(request, blueGreenFragmentId, {
      namespaceId,
      stateKey: "project.release_channel",
    });
    await expectFixtureMutationRejected(request, "00000000-0000-0000-0000-000000000999", {
      namespaceId,
      stateKey: "project.deployment_strategy",
    });
  }

  await page.getByRole("tab", { name: "Correct" }).click();
  await page.getByLabel("目标 Fragment ID", { exact: true }).fill(blueGreenFragmentId);
  await page.getByLabel("修正原因").fill("部署策略已人工复核");
  await page.getByLabel("修正内容").fill("部署策略修正为 canary");
  await page.getByRole("checkbox", { name: "附带 State declaration" }).check();
  await page.getByLabel("State JSON value").fill('"canary"');
  await page.getByLabel("State reason").fill("验证通过后切换为金丝雀发布");
  const correctResponse = page.waitForResponse((response) =>
    response.url().includes("/memory/correct") && response.request().method() === "POST",
  );
  await page.getByRole("button", { name: "提交修正" }).click();
  const correctEnvelope = await (await correctResponse).json();
  const canaryFragmentId = correctEnvelope.response.payload.fragment_id as string;
  expect(canaryFragmentId).not.toBe(blueGreenFragmentId);
  await expect(page.getByText("mutation", { exact: true })).toBeVisible();
  await expect(page.getByText(/"action": "correct"/)).toBeVisible();

  await page.getByRole("tab", { name: "Recall" }).click();
  await page.getByLabel("Recall Mode").selectOption("current_state");
  await page.getByLabel("State key").fill("project.deployment_strategy");
  const currentRecallResponse = page.waitForResponse((response) =>
    response.url().includes("/memory/recall") && response.request().method() === "POST",
  );
  await page.getByRole("button", { name: /Run Recall/ }).click();
  const currentRecallEnvelope = await (await currentRecallResponse).json();
  const currentQueryId = currentRecallEnvelope.response.payload.query_id as string;
  const canaryEvidence = currentRecallEnvelope.response.payload.current_states[0].evidence[0];
  const canaryEvidenceId = (canaryEvidence.evidence_ref_id ?? canaryEvidence.event_id) as string;
  const packet = page.getByLabel("Memory Packet");
  await expect(packet).toBeVisible();
  await expect(packet.locator(".packet-header code")).toHaveText(/^[0-9a-f-]{36}$/i);
  const currentStates = packet.getByRole("table", { name: "Current States" });
  await expect(currentStates).toBeVisible();
  await expect(currentStates.getByText("canary", { exact: true })).toBeVisible();
  await expect(currentStates.getByText("验证通过后切换为金丝雀发布", { exact: true })).toBeVisible();
  await expect(currentStates.getByText(canaryEvidenceId, { exact: true })).toBeVisible();

  await page.getByLabel("Recall Mode").selectOption("historical_state");
  const historicalRecallResponse = page.waitForResponse((response) =>
    response.url().includes("/memory/recall") && response.request().method() === "POST",
  );
  await page.getByRole("button", { name: /Run Recall/ }).click();
  const historicalRecallEnvelope = await (await historicalRecallResponse).json();
  const historicalQueryId = historicalRecallEnvelope.response.payload.query_id as string;
  expect(historicalQueryId).not.toBe(currentQueryId);
  const blueGreenState = historicalRecallEnvelope.response.payload.historical_states.find(
    (state: { source_event_id?: string }) => state.source_event_id === blueGreenEventId,
  );
  expect(blueGreenState.validity).toBe("superseded");
  const blueGreenEvidence = blueGreenState.evidence[0];
  const blueGreenEvidenceId = (blueGreenEvidence.evidence_ref_id ?? blueGreenEvidence.event_id) as string;
  const historicalStates = packet.getByRole("list", { name: "Historical States" });
  await expect(historicalStates).toBeVisible();
  const blueGreenHistory = historicalStates.locator("li").filter({ hasText: blueGreenEvidenceId });
  await expect(blueGreenHistory).toHaveCount(1);
  await expect(blueGreenHistory.getByText("blue_green", { exact: true })).toBeVisible();
  await expect(blueGreenHistory.getByText("生产环境先使用蓝绿发布", { exact: true })).toBeVisible();
  await expect(blueGreenHistory.getByText(blueGreenEvidenceId, { exact: true })).toBeVisible();
  const historicalTimes = await blueGreenHistory.locator("time").evaluateAll((times) => times.map((time) => ({
    dateTime: time.getAttribute("datetime"),
    text: time.textContent,
  })));
  expect(historicalTimes).toHaveLength(2);
  expect(historicalTimes.every(({ dateTime, text }) => Boolean(text?.trim()) && !Number.isNaN(Date.parse(dateTime ?? "")))).toBe(true);

  await page.getByRole("tab", { name: "Explain" }).click();
  await page.getByLabel("Query Text").fill("project.deployment_strategy");
  await page.getByRole("button", { name: /Run Explain/ }).click();
  await expect(page.getByText("explain", { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: "Forget" }).click();
  await page.getByLabel("目标 Fragment ID", { exact: true }).fill(canaryFragmentId);
  await page.getByLabel("失效原因").fill("fixture invalidation reason");
  const forget = page.getByRole("button", { name: "确认追加失效记录" });
  await expect(forget).toBeDisabled();
  await page.getByLabel("再次输入目标 Fragment ID").fill(canaryFragmentId);
  await expect(forget).toBeEnabled();
  await forget.click();
  await expect(page.getByText("mutation", { exact: true })).toBeVisible();
  await expect(page.getByText(/"action": "forget"/)).toBeVisible();

  await page.getByRole("tab", { name: "Recall" }).click();
  await page.getByLabel("Recall Mode").selectOption("current_state");
  await page.getByLabel("State key").fill("project.deployment_strategy");
  const forgottenRecallResponse = page.waitForResponse((response) =>
    response.url().includes("/memory/recall") && response.request().method() === "POST",
  );
  await page.getByRole("button", { name: /Run Recall/ }).click();
  const forgottenRecallEnvelope = await (await forgottenRecallResponse).json();
  const forgottenCanary = forgottenRecallEnvelope.response.payload.historical_states.find(
    (state: { evidence: Array<{ fragment_id: string }> }) =>
      state.evidence.some((evidence) => evidence.fragment_id === canaryFragmentId),
  );
  expect(forgottenRecallEnvelope.response.payload.current_states).toEqual([]);
  expect(forgottenCanary.validity).toBe("invalidated");
  await expect(packet.getByRole("heading", { name: "Current States 0" })).toBeVisible();
  await expect(packet.getByRole("table", { name: "Current States" })).toHaveCount(0);

  if (testInfo.project.name.includes("mobile")) {
    await page.getByRole("button", { name: "打开导航" }).click();
  }
  await page.getByRole("link", { name: "Agent Runtime" }).click();
  await expect(page.getByRole("heading", { name: "Agent Runtime" })).toBeVisible();
  await page.getByLabel("Runtime Prompt").fill("fixture runtime prompt");
  const runtimeStartResponse = page.waitForResponse((response) =>
    response.url().endsWith("/runtime/runs") && response.request().method() === "POST",
  );
  await page.getByRole("button", { name: "启动 Run" }).click();
  const runtimeStart = await (await runtimeStartResponse).json();
  const runId = runtimeStart.run_id as string;
  await expect(page.getByText("Fixture FinalResponse")).toBeVisible();

  await expect(page.getByLabel("Runtime Run 详情").getByText(runId, { exact: true })).toBeVisible();
  await expect(page.getByText("observing -> recalling")).toBeVisible();
  await expect(page.getByText("当前调用链未提供")).toBeVisible();
  await expect(page.getByText(/"phase": "observe"/).first()).toBeVisible();
  await expect(page.getByText("Approval approved")).toHaveCount(0);
  await expect(page.getByText("Tool executed")).toHaveCount(0);

  if (!isRealServer) {
    await page.waitForTimeout(500);
    const beforeRuntimeWindow = await fixtureRequests(request);
    await page.waitForTimeout(15_500);
    const afterRuntimeWindow = await fixtureRequests(request);
    expect(afterRuntimeWindow.health).toBe(beforeRuntimeWindow.health);
    expect(afterRuntimeWindow.ready).toBe(beforeRuntimeWindow.ready);
    expect(afterRuntimeWindow.metrics).toBe(beforeRuntimeWindow.metrics);
    expect(afterRuntimeWindow.stats).toBe(beforeRuntimeWindow.stats);
    expect(afterRuntimeWindow.runtimeList).toBeGreaterThan(beforeRuntimeWindow.runtimeList);
    expect(afterRuntimeWindow.runtimeDetail).toBeGreaterThan(beforeRuntimeWindow.runtimeDetail);
  }

  if (testInfo.project.name.includes("mobile")) {
    await page.getByRole("button", { name: "打开导航" }).click();
  }
  await page.getByRole("link", { name: "性能观测" }).click();
  await expect(page.getByRole("heading", { name: "性能观测" })).toBeVisible();
  await page.getByRole("tab", { name: "实时监控" }).click();
  await expect(page.getByText("实时流已连接")).toBeVisible();
  await expect(page.getByText("P95").first()).toBeVisible();
  await expect(page.getByText("429 背压", { exact: true })).toBeVisible();
  await expect(page.locator(".live-metrics strong").filter({ hasText: /req\/s$/ })).toBeVisible({ timeout: 12_000 });
  await expect.poll(async () => {
    const label = await page.locator("[aria-label^='样本 ']").getAttribute("aria-label");
    return Number(label?.match(/^样本 (\d+) \/ 60$/)?.[1] ?? 0);
  }).toBeGreaterThanOrEqual(2);
  await expect.poll(async () => page.locator(".recharts-line-curve").evaluateAll((paths) => paths.some((path) => {
    const line = path as SVGPathElement;
    return line.getTotalLength() > 0;
  }))).toBe(true);
  const timelineProgress = await page.locator(".recharts-line-curve").evaluateAll((paths) => {
    const line = paths
      .map((path) => path as SVGPathElement)
      .find((path) => path.getTotalLength() > 0);
    if (!line?.ownerSVGElement) return null;
    const start = line.getPointAtLength(0);
    const end = line.getPointAtLength(line.getTotalLength());
    return { startX: start.x, endX: end.x, width: line.ownerSVGElement.getBoundingClientRect().width };
  });
  expect(timelineProgress).not.toBeNull();
  expect(timelineProgress!.endX).toBeGreaterThan(timelineProgress!.startX);
  expect((timelineProgress!.endX - timelineProgress!.startX) / timelineProgress!.width).toBeLessThan(0.15);

  if (!isRealServer) {
    await page.waitForTimeout(500);
    const beforePerformanceWindow = await fixtureRequests(request);
    const positionsBeforeFailure = await performancePositions(page);
    await setMetricsFailure(request, true);
    await expect(page.getByText("指标采样失败，图表保留最后成功数据且缺口不会补零。")).toBeVisible();
    const positionsDuringFailure = await performancePositions(page);
    expect(positionsDuringFailure.monitorTop).toBeCloseTo(positionsBeforeFailure.monitorTop, 1);
    expect(positionsDuringFailure.chartTop).toBeCloseTo(positionsBeforeFailure.chartTop, 1);
    await setMetricsFailure(request, false);
    await expect(page.getByTestId("metrics-channel-status")).toHaveClass(/is-empty/);
    const positionsAfterRecovery = await performancePositions(page);
    expect(positionsAfterRecovery.monitorTop).toBeCloseTo(positionsBeforeFailure.monitorTop, 1);
    expect(positionsAfterRecovery.chartTop).toBeCloseTo(positionsBeforeFailure.chartTop, 1);
    await page.waitForTimeout(5_500);
    const afterPerformanceWindow = await fixtureRequests(request);
    expect(afterPerformanceWindow.metrics).toBeGreaterThan(beforePerformanceWindow.metrics);
    expect(afterPerformanceWindow.runtimeList).toBe(beforePerformanceWindow.runtimeList);
    expect(afterPerformanceWindow.runtimeDetail).toBe(beforePerformanceWindow.runtimeDetail);
  }

  const websocketUrls = await page.evaluate(() =>
    (window as typeof window & { __instanceStreamUrls?: string[] }).__instanceStreamUrls ?? []
  );
  const streamUrls = websocketUrls.filter((url) => url.includes("/api/v1/instances/"));
  expect(streamUrls).toHaveLength(1);
  expect(streamUrls[0]).toContain("/api/v1/instances/");
  expect(streamUrls[0]).toContain("/stream");
  expect(streamUrls[0]).not.toContain("/metrics/stream");
  expect(legacyBrowserReads(browserRequests)).toEqual([]);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: testInfo.outputPath("performance-live.png"), fullPage: true });
  await page.getByRole("tab", { name: "静态报告" }).click();
  const reportInput = page.getByRole("button", { name: "导入压测报告" });
  await reportInput.setInputFiles(performanceFixture);
  await expect(page.getByRole("rowheader", { name: "recall-fixture", exact: true })).toBeVisible();
  await expect(page.getByText("P95").first()).toBeVisible();
  await expect(page.getByText("429 背压", { exact: true })).toBeVisible();

  expect(apiResponses.join("\n")).not.toContain(apiKey);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: testInfo.outputPath("control-plane.png"), fullPage: true });
});

interface FixtureRequestCounts {
  health: number;
  ready: number;
  metrics: number;
  stats: number;
  runtimeList: number;
  runtimeDetail: number;
  runtimeStart: number;
}

async function fixtureRequests(request: APIRequestContext): Promise<FixtureRequestCounts> {
  const response = await request.get(`http://127.0.0.1:${memoryPort}/__fixture/requests`);
  expect(response.ok()).toBe(true);
  return response.json() as Promise<FixtureRequestCounts>;
}

async function setMetricsFailure(request: APIRequestContext, enabled: boolean): Promise<void> {
  const response = await request.post(`http://127.0.0.1:${memoryPort}/__fixture/metrics-mode`, {
    data: { failure: enabled },
  });
  expect(response.ok()).toBe(true);
}

async function expectFixtureMutationRejected(
  request: APIRequestContext,
  targetFragmentId: string,
  input: { namespaceId: string; stateKey: string },
): Promise<void> {
  const response = await request.post(`http://127.0.0.1:${memoryPort}/v1/memory/correct`, {
    headers: { "x-api-key": apiKey },
    data: {
      api_version: "v1",
      request_id: crypto.randomUUID(),
      access_scope: {
        namespace_id: input.namespaceId,
        tenant_id: null,
        user_id: null,
        agent_id: null,
        project_id: null,
      },
      command: {
        operation: "correct",
        payload: {
          target_fragment_id: targetFragmentId,
          reason: "验证 Fixture 拒绝非法修正目标",
          replacement: { text: "不会写入的修正内容" },
          state: {
            key: input.stateKey,
            value: "canary",
            reason: "不会写入的 State",
          },
          occurred_at: new Date().toISOString(),
        },
      },
    },
  });
  expect(response.status()).toBe(400);
}

async function performancePositions(page: Page): Promise<{ monitorTop: number; chartTop: number }> {
  return page.evaluate(() => {
    const monitor = document.querySelector<HTMLElement>(".live-monitor");
    const charts = document.querySelector<HTMLElement>(".performance-chart-grid");
    if (!monitor || !charts) throw new Error("实时性能布局尚未渲染");
    return {
      monitorTop: monitor.getBoundingClientRect().top,
      chartTop: charts.getBoundingClientRect().top,
    };
  });
}

function legacyBrowserReads(requests: Array<{ method: string; url: string }>) {
  return requests.filter(({ method, url }) => {
    if (!url.includes("/api/v1/instances/")) return false;
    if (url.includes("/memory/stats")) return true;
    if (method !== "GET") return false;
    return /\/status(?:\?|$)|\/runtime\/runs(?:[/?]|$)/.test(url);
  });
}
