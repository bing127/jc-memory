import cookie from "@fastify/cookie";
import Fastify from "fastify";
import { afterEach, describe, expect, test } from "vitest";

import {
  createAuthService,
  registerSessionRoutes,
} from "../src/auth.js";

const apps: ReturnType<typeof Fastify>[] = [];
const origin = "http://127.0.0.1:5173";

afterEach(async () => {
  await Promise.all(apps.splice(0).map((app) => app.close()));
});

describe("control-plane session", () => {
  test("rejects invalid passwords and never puts the password in the cookie", async () => {
    const app = await buildApp();
    const denied = await app.inject({
      method: "POST",
      url: "/api/v1/session",
      headers: { origin },
      payload: { password: "wrong-password" },
    });
    expect(denied.statusCode).toBe(401);
    expect(denied.json()).toMatchObject({ code: "unauthorized" });

    const accepted = await login(app);
    const setCookie = accepted.headers["set-cookie"];
    expect(setCookie).toContain("HttpOnly");
    expect(setCookie).toContain("SameSite=Strict");
    expect(setCookie).not.toContain("correct-password");
  });

  test("accepts a signed session and rejects a tampered token", async () => {
    const app = await buildApp();
    const accepted = await login(app);
    const cookieHeader = cookiePair(accepted.headers["set-cookie"]);

    const session = await app.inject({
      method: "GET",
      url: "/api/v1/session",
      headers: { cookie: cookieHeader },
    });
    expect(session.statusCode).toBe(200);
    expect(session.json()).toEqual({ authenticated: true, subject: "owner" });

    const tampered = await app.inject({
      method: "GET",
      url: "/api/v1/session",
      headers: { cookie: `${cookieHeader}tampered` },
    });
    expect(tampered.statusCode).toBe(401);
  });

  test("rejects state-changing requests from an untrusted origin", async () => {
    const app = await buildApp();
    const response = await app.inject({
      method: "POST",
      url: "/api/v1/session",
      headers: { origin: "https://attacker.example" },
      payload: { password: "correct-password" },
    });

    expect(response.statusCode).toBe(403);
    expect(response.json()).toMatchObject({ code: "origin_not_allowed" });
  });
});

async function buildApp() {
  const app = Fastify({ logger: false });
  apps.push(app);
  await app.register(cookie);
  const auth = createAuthService({
    adminPassword: "correct-password",
    sessionSecret: Buffer.alloc(32, 9),
    allowedOrigins: [origin],
    secureCookies: false,
    sessionTtlMs: 60_000,
  });
  await registerSessionRoutes(app, auth);
  return app;
}

async function login(app: ReturnType<typeof Fastify>) {
  return app.inject({
    method: "POST",
    url: "/api/v1/session",
    headers: { origin },
    payload: { password: "correct-password" },
  });
}

function cookiePair(value: string | string[] | undefined): string {
  if (!value) {
    throw new Error("missing Set-Cookie response header");
  }
  const header = Array.isArray(value) ? value[0] : value;
  if (!header) {
    throw new Error("empty Set-Cookie response header");
  }
  return header.split(";", 1)[0] ?? "";
}
