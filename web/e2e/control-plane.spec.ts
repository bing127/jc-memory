import { expect, test } from "@playwright/test";

const namespaceId = "00000000-0000-0000-0000-000000000001";
const apiKey = "fixture-instance-api-key";
const memoryPort = Number(process.env.JC_WEB_E2E_MEMORY_PORT ?? 8787);

test("login, connect and execute the complete Memory Console path through the BFF", async ({ page }, testInfo) => {
  const apiResponses: string[] = [];
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
  await expect(page.getByText("server_ready")).toBeVisible();
  if (testInfo.project.name.includes("mobile")) {
    await page.getByRole("button", { name: "打开导航" }).click();
  }
  await page.getByRole("link", { name: "Memory Lab" }).click();
  await expect(page.getByRole("heading", { name: "Memory Lab" })).toBeVisible();

  await page.getByLabel("原始文本").fill("fixture web observation");
  const observeResponse = page.waitForResponse((response) =>
    response.url().includes("/memory/observe") && response.request().method() === "POST",
  );
  await page.getByRole("button", { name: /Observe Fragment/ }).click();
  const observeEnvelope = await (await observeResponse).json();
  const targetFragmentId = observeEnvelope.response.payload.fragment_id as string;
  await expect(page.getByText("observe", { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: "Commit" }).click();
  await page.getByLabel("提交文本").fill("fixture committed fragment");
  await page.getByRole("button", { name: /Commit Fragment/ }).click();
  await expect(page.getByText("commit", { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: "Recall" }).click();
  await page.getByLabel("Recall Mode").selectOption("exact");
  await page.getByLabel("Query Text").fill("fixture web observation");
  await page.getByRole("button", { name: /Run Recall/ }).click();
  const packet = page.getByLabel("Memory Packet");
  await expect(packet).toBeVisible();
  await expect(packet.locator(".packet-header code")).toHaveText(/^[0-9a-f-]{36}$/i);
  await expect(packet.locator("pre").filter({ hasText: "fixture web observation" }).first()).toBeVisible();

  await page.getByRole("tab", { name: "Explain" }).click();
  await page.getByLabel("Query Text").fill("fixture web observation");
  await page.getByRole("button", { name: /Run Explain/ }).click();
  await expect(page.getByText("explain", { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: "Correct" }).click();
  await page.getByLabel("目标 Fragment ID", { exact: true }).fill(targetFragmentId);
  await page.getByLabel("修正原因").fill("fixture correction reason");
  await page.getByLabel("修正内容").fill("fixture corrected content");
  await page.getByRole("button", { name: "提交修正" }).click();
  await expect(page.getByText("mutation", { exact: true })).toBeVisible();
  await expect(page.getByText(/"action": "correct"/)).toBeVisible();

  await page.getByRole("tab", { name: "Forget" }).click();
  await page.getByLabel("目标 Fragment ID", { exact: true }).fill(targetFragmentId);
  await page.getByLabel("失效原因").fill("fixture invalidation reason");
  const forget = page.getByRole("button", { name: "确认追加失效记录" });
  await expect(forget).toBeDisabled();
  await page.getByLabel("再次输入目标 Fragment ID").fill(targetFragmentId);
  await expect(forget).toBeEnabled();
  await forget.click();
  await expect(page.getByText("mutation", { exact: true })).toBeVisible();
  await expect(page.getByText(/"action": "forget"/)).toBeVisible();

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

  expect(apiResponses.join("\n")).not.toContain(apiKey);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: testInfo.outputPath("control-plane.png"), fullPage: true });
});
