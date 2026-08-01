import { defineConfig, devices } from "@playwright/test";

const frontendPort = Number(process.env.JC_WEB_E2E_FRONTEND_PORT ?? 5173);
const backendPort = Number(process.env.JC_WEB_E2E_BACKEND_PORT ?? 8790);
const memoryPort = Number(process.env.JC_WEB_E2E_MEMORY_PORT ?? 8787);
const grpcPort = Number(process.env.JC_WEB_E2E_GRPC_PORT ?? 8788);
const providerPort = Number(process.env.JC_WEB_E2E_PROVIDER_PORT ?? 8789);
const useRealServer = process.env.JC_WEB_E2E_REAL_SERVER === "1";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  retries: 0,
  reporter: "line",
  use: {
    baseURL: `http://127.0.0.1:${frontendPort}`,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: [
    {
      command: useRealServer
        ? "node e2e/start-real-memory-server.mjs"
        : "node e2e/fixture-memory-server.mjs",
      url: `http://127.0.0.1:${memoryPort}/healthz`,
      reuseExistingServer: false,
      timeout: useRealServer ? 120_000 : 60_000,
      env: {
        JC_WEB_E2E_MEMORY_PORT: String(memoryPort),
        JC_WEB_E2E_GRPC_PORT: String(grpcPort),
        JC_WEB_E2E_PROVIDER_PORT: String(providerPort),
      },
    },
    {
      command: "pnpm --filter @jiangcheng-memory/control-plane-backend exec node --import tsx ../e2e/start-backend.mjs",
      url: `http://127.0.0.1:${backendPort}/healthz`,
      reuseExistingServer: false,
      env: {
        JC_WEB_E2E_BACKEND_PORT: String(backendPort),
        JC_WEB_E2E_FRONTEND_PORT: String(frontendPort),
      },
    },
    {
      command: `pnpm --filter @jiangcheng-memory/control-plane-frontend exec vite --host 127.0.0.1 --port ${frontendPort}`,
      url: `http://127.0.0.1:${frontendPort}`,
      reuseExistingServer: false,
      env: { JC_WEB_E2E_BACKEND_PORT: String(backendPort) },
    },
  ],
  projects: [
    {
      name: "desktop-chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } },
    },
    {
      name: "mobile-chromium",
      use: { ...devices["Pixel 7"], viewport: { width: 390, height: 844 } },
    },
  ],
});
