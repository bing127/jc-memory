import { spawn } from "node:child_process";
import { createServer } from "node:net";

const input = process.argv.slice(2);
const realIndex = input.indexOf("--real");
const useRealServer = realIndex >= 0;
if (useRealServer) input.splice(realIndex, 1);

const environment = { ...process.env };
for (const name of [
  "JC_WEB_E2E_FRONTEND_PORT",
  "JC_WEB_E2E_BACKEND_PORT",
  "JC_WEB_E2E_MEMORY_PORT",
  "JC_WEB_E2E_GRPC_PORT",
  "JC_WEB_E2E_PROVIDER_PORT",
]) {
  environment[name] ??= String(await availablePort());
}
if (useRealServer) environment.JC_WEB_E2E_REAL_SERVER = "1";

const command = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const child = spawn(command, ["exec", "playwright", "test", ...input], {
  env: environment,
  stdio: "inherit",
});
child.once("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  process.exit(code ?? 1);
});
child.once("error", (error) => {
  console.error(error);
  process.exit(1);
});

function availablePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close();
        reject(new Error("无法分配 E2E 隔离端口"));
        return;
      }
      server.close((error) => error ? reject(error) : resolve(address.port));
    });
  });
}
