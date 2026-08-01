import { rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const backendPort = Number(process.env.JC_WEB_E2E_BACKEND_PORT ?? 8790);
const frontendPort = Number(process.env.JC_WEB_E2E_FRONTEND_PORT ?? 5173);
const databasePath = join(root, "e2e", `control-plane-e2e-${backendPort}.db`);
for (const path of [databasePath, `${databasePath}-shm`, `${databasePath}-wal`]) {
  await rm(path, { force: true });
}

process.env.JC_WEB_DATABASE_PATH = databasePath;
process.env.JC_WEB_ADMIN_PASSWORD = "e2e-control-plane-password";
process.env.JC_WEB_SESSION_SECRET = Buffer.alloc(32, 11).toString("base64");
process.env.JC_WEB_CREDENTIAL_KEY = Buffer.alloc(32, 12).toString("base64");
process.env.JC_WEB_ALLOWED_ORIGINS = `http://127.0.0.1:${frontendPort}`;
process.env.JC_WEB_BIND_HOST = "127.0.0.1";
process.env.JC_WEB_BIND_PORT = String(backendPort);
process.env.NODE_ENV = "test";

await import("../backend/src/server.ts");
