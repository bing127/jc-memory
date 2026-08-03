import cookie from "@fastify/cookie";
import cors from "@fastify/cors";
import websocket from "@fastify/websocket";
import Fastify, { type FastifyInstance } from "fastify";

import { createAuthService, registerSessionRoutes } from "./auth.js";
import type { ControlPlaneConfig } from "./config.js";
import { createCredentialCipher } from "./crypto.js";
import { openControlPlaneDatabase } from "./database.js";
import { createInstanceStore } from "./instances.js";
import { registerInstanceRoutes } from "./routes/instances.js";
import { registerMemoryRoutes } from "./routes/memory.js";
import { registerRuntimeRoutes } from "./routes/runtime.js";
import { UpstreamError, createUpstreamClient } from "./upstream.js";

interface BuildAppOptions {
  config: ControlPlaneConfig;
  fetch?: typeof globalThis.fetch;
  logger?: boolean;
}

export async function buildApp(options: BuildAppOptions): Promise<FastifyInstance> {
  const app = Fastify({
    logger:
      options.logger === false
        ? false
        : {
            redact: {
              paths: [
                "req.headers.authorization",
                "req.headers.x-api-key",
                "req.headers.cookie",
                "res.headers.set-cookie",
                "body.apiKey",
                "password",
              ],
              censor: "[REDACTED]",
            },
          },
  });
  const database = openControlPlaneDatabase(options.config.databasePath);
  const cipher = createCredentialCipher(options.config.credentialKey);
  const store = createInstanceStore(database, cipher);
  const auth = createAuthService(options.config);
  const upstream = createUpstreamClient(
    options.fetch
      ? { fetch: options.fetch, timeoutMs: options.config.upstreamTimeoutMs }
      : { timeoutMs: options.config.upstreamTimeoutMs },
  );

  app.addHook("onClose", async () => database.close());
  await app.register(cookie);
  await app.register(websocket);
  await app.register(cors, {
    origin: options.config.allowedOrigins,
    credentials: true,
    methods: ["GET", "POST", "DELETE"],
  });
  app.get("/healthz", async () => ({ status: "ok" }));
  await registerSessionRoutes(app, auth);
  await registerInstanceRoutes(app, auth, store, upstream, options.config.streamIntervals);
  await registerMemoryRoutes(app, auth, store, upstream);
  await registerRuntimeRoutes(app, auth, store, upstream);

  app.setErrorHandler((error, _request, reply) => {
    if (error instanceof UpstreamError) {
      const publicError = error.toPublic();
      return reply.code(publicError.status).send(publicError);
    }
    app.log.error({ err: error }, "control-plane request failed");
    return reply.code(500).send({
      code: "internal",
      message: "控制面请求失败",
    });
  });

  return app;
}
