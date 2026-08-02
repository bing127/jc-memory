import { randomUUID } from "node:crypto";
import type WebSocket from "ws";
import { z } from "zod";

import type { InstanceConnection } from "./instances.js";
import { parseServerMetrics } from "./metrics.js";
import { uuidStringSchema } from "./protocol.js";
import type { UpstreamClient } from "./upstream.js";
import { toAccessScope } from "./routes/memory.js";

const topicSchema = z.enum(["overview", "metrics", "runtime.list", "runtime.detail"]);
const subscriptionSchema = z.discriminatedUnion("topic", [
  z.strictObject({ topic: z.literal("overview") }),
  z.strictObject({ topic: z.literal("metrics") }),
  z.strictObject({ topic: z.literal("runtime.list") }),
  z.strictObject({ topic: z.literal("runtime.detail"), runId: uuidStringSchema }),
]);
const clientMessageSchema = z.discriminatedUnion("type", [
  z.strictObject({
    version: z.literal(1),
    type: z.literal("subscribe"),
    subscriptions: z.array(subscriptionSchema).max(4),
  }),
  z.strictObject({
    version: z.literal(1),
    type: z.literal("refresh"),
    topics: z.array(topicSchema).min(1).max(4),
  }),
]);

type Topic = z.infer<typeof topicSchema>;
type Subscription = z.infer<typeof subscriptionSchema>;

interface StreamTask {
  subscription: Subscription;
  timer: ReturnType<typeof setInterval> | null;
  sampling: boolean;
  closed: boolean;
  failed: boolean;
}

const topicIntervals: Record<Topic, number> = {
  overview: 15_000,
  metrics: 1_000,
  "runtime.list": 10_000,
  "runtime.detail": 5_000,
};

export function attachInstanceStream(
  socket: WebSocket,
  connection: InstanceConnection,
  upstream: UpstreamClient,
): void {
  const tasks = new Map<Topic, StreamTask>();
  let closed = false;

  socket.on("message", (payload) => {
    let value: unknown;
    try {
      value = JSON.parse(payload.toString());
    } catch {
      sendProtocolError("invalid_json", "订阅消息必须是合法 JSON");
      return;
    }
    const parsed = clientMessageSchema.safeParse(value);
    if (!parsed.success) {
      sendProtocolError("invalid_subscription", "订阅消息格式无效");
      return;
    }
    if (parsed.data.type === "subscribe") {
      applySubscriptions(parsed.data.subscriptions);
      return;
    }
    refreshTopics(parsed.data.topics);
  });

  const stop = () => {
    if (closed) return;
    closed = true;
    for (const task of tasks.values()) stopTask(task);
    tasks.clear();
  };
  socket.once("close", stop);
  socket.once("error", stop);

  function applySubscriptions(subscriptions: Subscription[]): void {
    const desired = new Map<Topic, Subscription>();
    for (const subscription of subscriptions) {
      if (desired.has(subscription.topic)) {
        sendProtocolError("invalid_subscription", "同一 Topic 只能订阅一次");
        return;
      }
      desired.set(subscription.topic, subscription);
    }

    for (const [topic, task] of tasks) {
      const next = desired.get(topic);
      if (!next || subscriptionKey(next) !== subscriptionKey(task.subscription)) {
        stopTask(task);
        tasks.delete(topic);
      }
    }
    for (const [topic, subscription] of desired) {
      if (tasks.has(topic)) continue;
      const task = startTask(subscription);
      tasks.set(topic, task);
      void sample(task);
    }
  }

  function refreshTopics(topics: Topic[]): void {
    for (const topic of new Set(topics)) {
      const task = tasks.get(topic);
      if (!task) {
        sendProtocolError("topic_not_subscribed", `Topic ${topic} 尚未订阅`);
        continue;
      }
      void sample(task);
    }
  }

  function startTask(subscription: Subscription): StreamTask {
    const task: StreamTask = {
      subscription,
      timer: null,
      sampling: false,
      closed: false,
      failed: false,
    };
    task.timer = setInterval(() => void sample(task), topicIntervals[subscription.topic]);
    task.timer.unref?.();
    return task;
  }

  function stopTask(task: StreamTask): void {
    if (task.closed) return;
    task.closed = true;
    if (task.timer) clearInterval(task.timer);
  }

  async function sample(task: StreamTask): Promise<void> {
    if (closed || task.closed || task.sampling || socket.readyState !== 1) return;
    task.sampling = true;
    const sampledAt = new Date().toISOString();
    try {
      const data = await sampleTopic(task.subscription, connection, upstream, sampledAt);
      if (!closed && !task.closed && socket.readyState === 1) {
        task.failed = false;
        socket.send(JSON.stringify({
          version: 1,
          type: "snapshot",
          topic: task.subscription.topic,
          sampledAt,
          data,
        }));
      }
    } catch {
      if (!closed && !task.closed && socket.readyState === 1 && !task.failed) {
        task.failed = true;
        socket.send(JSON.stringify({
          version: 1,
          type: "channel_error",
          topic: task.subscription.topic,
          sampledAt,
          code: "upstream_unavailable",
          message: channelErrorMessage(task.subscription.topic),
        }));
      }
    } finally {
      task.sampling = false;
    }
  }

  function sendProtocolError(code: string, message: string): void {
    if (closed || socket.readyState !== 1) return;
    socket.send(JSON.stringify({ version: 1, type: "protocol_error", code, message }));
  }
}

async function sampleTopic(
  subscription: Subscription,
  connection: InstanceConnection,
  upstream: UpstreamClient,
  sampledAt: string,
): Promise<unknown> {
  if (subscription.topic === "metrics") {
    return parseServerMetrics(await upstream.metrics(connection), sampledAt);
  }
  if (subscription.topic === "runtime.list") {
    return upstream.runtimeRuns(connection, { limit: 25, offset: 0 });
  }
  if (subscription.topic === "runtime.detail") {
    return upstream.runtimeRun(connection, subscription.runId);
  }
  const [status, stats] = await Promise.all([
    upstream.status(connection),
    upstream.execute(connection, {
      operation: "stats",
      requestId: randomUUID(),
      accessScope: toAccessScope(connection.namespaceId, {
        tenantId: null,
        userId: null,
        agentId: null,
        projectId: null,
      }),
      payload: {},
    }),
  ]);
  return {
    status: {
      health: status.health,
      ready: status.ready,
      serverReady: /^jc_memory_server_ready 1$/m.test(status.metrics),
    },
    stats,
  };
}

function subscriptionKey(subscription: Subscription): string {
  return subscription.topic === "runtime.detail"
    ? `${subscription.topic}:${subscription.runId}`
    : subscription.topic;
}

function channelErrorMessage(topic: Topic): string {
  if (topic === "metrics") return "实例指标采样失败";
  if (topic === "overview") return "实例总览采样失败";
  if (topic === "runtime.list") return "Runtime 列表采样失败";
  return "Runtime 详情采样失败";
}
