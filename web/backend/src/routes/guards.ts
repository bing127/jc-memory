import type { FastifyReply, FastifyRequest } from "fastify";

import type { AuthService } from "../auth.js";

export function requireSession(
  request: FastifyRequest,
  reply: FastifyReply,
  auth: AuthService,
): boolean {
  if (auth.sessionFromRequest(request)) {
    return true;
  }
  void reply.code(401).send({
    code: "unauthorized",
    message: "会话不存在或已经过期",
  });
  return false;
}

export function requireAllowedOrigin(
  request: FastifyRequest,
  reply: FastifyReply,
  auth: AuthService,
): boolean {
  if (auth.isAllowedOrigin(request.headers.origin)) {
    return true;
  }
  void reply.code(403).send({
    code: "origin_not_allowed",
    message: "请求来源不在控制面允许列表中",
  });
  return false;
}
