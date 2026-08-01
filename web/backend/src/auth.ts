import {
  createHmac,
  scryptSync,
  timingSafeEqual,
} from "node:crypto";
import type { FastifyInstance, FastifyRequest } from "fastify";
import { z } from "zod";

export const sessionCookieName = "jc_memory_session";

interface AuthConfig {
  adminPassword: string;
  sessionSecret: Buffer;
  allowedOrigins: string[];
  secureCookies: boolean;
  sessionTtlMs: number;
}

interface SessionPayload {
  subject: "owner";
  issuedAt: number;
  expiresAt: number;
  nonce: string;
}

export interface AuthService {
  readonly secureCookies: boolean;
  readonly sessionTtlMs: number;
  isAllowedOrigin(origin: string | undefined): boolean;
  verifyPassword(password: string): boolean;
  createSession(): string;
  verifySession(token: string | undefined): SessionPayload | null;
  sessionFromRequest(request: FastifyRequest): SessionPayload | null;
}

const loginSchema = z.strictObject({
  password: z.string().min(1).max(256),
});

export function createAuthService(config: AuthConfig): AuthService {
  if (config.sessionSecret.byteLength < 32) {
    throw new Error("session secret must contain at least 32 bytes");
  }
  if (config.sessionTtlMs <= 0) {
    throw new Error("session TTL must be greater than zero");
  }

  const origins = new Set(config.allowedOrigins);
  const passwordSalt = createHmac("sha256", config.sessionSecret)
    .update("jiangcheng-memory-admin-password")
    .digest()
    .subarray(0, 16);
  const expectedPassword = derivePassword(config.adminPassword, passwordSalt);

  function sign(payload: string): string {
    return createHmac("sha256", config.sessionSecret)
      .update(payload)
      .digest("base64url");
  }

  function verifySession(token: string | undefined): SessionPayload | null {
    if (!token) {
      return null;
    }
    const parts = token.split(".");
    if (parts.length !== 2) {
      return null;
    }
    const [encodedPayload, encodedSignature] = parts;
    if (!encodedPayload || !encodedSignature) {
      return null;
    }
    const expectedSignature = sign(encodedPayload);
    const actualBuffer = Buffer.from(encodedSignature);
    const expectedBuffer = Buffer.from(expectedSignature);
    if (
      actualBuffer.byteLength !== expectedBuffer.byteLength ||
      !timingSafeEqual(actualBuffer, expectedBuffer)
    ) {
      return null;
    }

    try {
      const candidate = JSON.parse(
        Buffer.from(encodedPayload, "base64url").toString("utf8"),
      ) as Partial<SessionPayload>;
      if (
        candidate.subject !== "owner" ||
        typeof candidate.issuedAt !== "number" ||
        typeof candidate.expiresAt !== "number" ||
        typeof candidate.nonce !== "string" ||
        candidate.expiresAt <= Date.now()
      ) {
        return null;
      }
      return candidate as SessionPayload;
    } catch {
      return null;
    }
  }

  return {
    secureCookies: config.secureCookies,
    sessionTtlMs: config.sessionTtlMs,
    isAllowedOrigin(origin) {
      return origin !== undefined && origins.has(origin);
    },
    verifyPassword(password) {
      const actual = derivePassword(password, passwordSalt);
      return timingSafeEqual(actual, expectedPassword);
    },
    createSession() {
      const issuedAt = Date.now();
      const payload: SessionPayload = {
        subject: "owner",
        issuedAt,
        expiresAt: issuedAt + config.sessionTtlMs,
        nonce: crypto.randomUUID(),
      };
      const encodedPayload = Buffer.from(JSON.stringify(payload)).toString(
        "base64url",
      );
      return `${encodedPayload}.${sign(encodedPayload)}`;
    },
    verifySession,
    sessionFromRequest(request) {
      return verifySession(request.cookies[sessionCookieName]);
    },
  };
}

export async function registerSessionRoutes(
  app: FastifyInstance,
  auth: AuthService,
): Promise<void> {
  app.post("/api/v1/session", async (request, reply) => {
    if (!auth.isAllowedOrigin(request.headers.origin)) {
      return reply.code(403).send({
        code: "origin_not_allowed",
        message: "请求来源不在控制面允许列表中",
      });
    }
    const parsed = loginSchema.safeParse(request.body);
    if (!parsed.success || !auth.verifyPassword(parsed.data.password)) {
      return reply.code(401).send({
        code: "unauthorized",
        message: "管理员密码无效",
      });
    }

    reply.setCookie(sessionCookieName, auth.createSession(), {
      path: "/",
      httpOnly: true,
      sameSite: "strict",
      secure: auth.secureCookies,
      maxAge: Math.floor(auth.sessionTtlMs / 1000),
    });
    return { authenticated: true, subject: "owner" };
  });

  app.get("/api/v1/session", async (request, reply) => {
    const session = auth.sessionFromRequest(request);
    if (!session) {
      return reply.code(401).send({
        code: "unauthorized",
        message: "会话不存在或已经过期",
      });
    }
    return { authenticated: true, subject: session.subject };
  });

  app.delete("/api/v1/session", async (request, reply) => {
    if (!auth.isAllowedOrigin(request.headers.origin)) {
      return reply.code(403).send({
        code: "origin_not_allowed",
        message: "请求来源不在控制面允许列表中",
      });
    }
    reply.clearCookie(sessionCookieName, { path: "/" });
    return reply.code(204).send();
  });
}

function derivePassword(password: string, salt: Buffer): Buffer {
  return scryptSync(password, salt, 32, {
    N: 16_384,
    r: 8,
    p: 1,
    maxmem: 64 * 1024 * 1024,
  });
}
