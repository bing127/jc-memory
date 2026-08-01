import { resolve } from "node:path";
import { z } from "zod";

export interface ControlPlaneConfig {
  databasePath: string;
  adminPassword: string;
  sessionSecret: Buffer;
  credentialKey: Buffer;
  allowedOrigins: string[];
  secureCookies: boolean;
  sessionTtlMs: number;
  bindHost: string;
  bindPort: number;
}

const environmentSchema = z.object({
  JC_WEB_DATABASE_PATH: z.string().min(1).default("./data/web/control-plane.db"),
  JC_WEB_ADMIN_PASSWORD: z.string().min(12).max(256),
  JC_WEB_SESSION_SECRET: z.string().min(1),
  JC_WEB_CREDENTIAL_KEY: z.string().min(1),
  JC_WEB_ALLOWED_ORIGINS: z
    .string()
    .default("http://127.0.0.1:5173,http://localhost:5173"),
  JC_WEB_SESSION_TTL_MS: z.coerce.number().int().positive().default(28_800_000),
  JC_WEB_BIND_HOST: z.string().min(1).default("127.0.0.1"),
  JC_WEB_BIND_PORT: z.coerce.number().int().min(1).max(65_535).default(8790),
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
});

export function loadConfig(
  environment: NodeJS.ProcessEnv = process.env,
): ControlPlaneConfig {
  const parsed = environmentSchema.parse(environment);
  const sessionSecret = decodeKey(parsed.JC_WEB_SESSION_SECRET, "JC_WEB_SESSION_SECRET");
  const credentialKey = decodeKey(parsed.JC_WEB_CREDENTIAL_KEY, "JC_WEB_CREDENTIAL_KEY", true);
  const allowedOrigins = parsed.JC_WEB_ALLOWED_ORIGINS.split(",")
    .map((origin) => origin.trim())
    .filter(Boolean);
  if (allowedOrigins.length === 0) {
    throw new Error("JC_WEB_ALLOWED_ORIGINS must contain at least one origin");
  }

  return {
    databasePath: resolve(parsed.JC_WEB_DATABASE_PATH),
    adminPassword: parsed.JC_WEB_ADMIN_PASSWORD,
    sessionSecret,
    credentialKey,
    allowedOrigins,
    secureCookies: parsed.NODE_ENV === "production",
    sessionTtlMs: parsed.JC_WEB_SESSION_TTL_MS,
    bindHost: parsed.JC_WEB_BIND_HOST,
    bindPort: parsed.JC_WEB_BIND_PORT,
  };
}

function decodeKey(value: string, name: string, exact = false): Buffer {
  const decoded = Buffer.from(value, "base64");
  if ((exact && decoded.byteLength !== 32) || (!exact && decoded.byteLength < 32)) {
    throw new Error(`${name} must decode to ${exact ? "exactly " : "at least "}32 bytes`);
  }
  return decoded;
}
