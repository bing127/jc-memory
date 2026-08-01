import type {
  InstanceStreamSubscription,
  InstanceStreamTopic,
  InstanceStreamTopicState,
  InstanceTransportState,
} from "./contracts.js";

interface InstanceStreamSnapshot {
  transportState: InstanceTransportState;
  topics: Record<InstanceStreamTopic, InstanceStreamTopicState>;
}

interface RegistryEntry {
  client: InstanceStreamClient;
  references: number;
  releaseTimer: ReturnType<typeof setTimeout> | null;
}

const registry = new Map<string, RegistryEntry>();
const reconnectBaseDelayMs = 1_000;
const reconnectMaximumDelayMs = 30_000;

export function acquireInstanceStreamClient(instanceId: string): InstanceStreamClient {
  const existing = registry.get(instanceId);
  if (existing) {
    existing.references += 1;
    if (existing.releaseTimer) clearTimeout(existing.releaseTimer);
    existing.releaseTimer = null;
    return existing.client;
  }
  const client = new InstanceStreamClient(instanceId);
  registry.set(instanceId, { client, references: 1, releaseTimer: null });
  return client;
}

export function releaseInstanceStreamClient(instanceId: string, client: InstanceStreamClient): void {
  const entry = registry.get(instanceId);
  if (!entry || entry.client !== client) return;
  entry.references = Math.max(0, entry.references - 1);
  if (entry.references > 0 || entry.releaseTimer) return;
  entry.releaseTimer = setTimeout(() => {
    const current = registry.get(instanceId);
    if (!current || current.client !== client || current.references > 0) return;
    current.client.dispose();
    registry.delete(instanceId);
  }, 0);
}

export class InstanceStreamClient {
  private socket: WebSocket | null = null;
  private subscriptions: InstanceStreamSubscription[] = [];
  private subscriptionSignature = "[]";
  private reconnectAttempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;
  private snapshot: InstanceStreamSnapshot = {
    transportState: "connecting",
    topics: emptyTopicStates(),
  };
  private readonly listeners = new Set<() => void>();

  constructor(private readonly instanceId: string) {
    this.connect();
  }

  getSnapshot(): InstanceStreamSnapshot {
    return this.snapshot;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  setSubscriptions(subscriptions: InstanceStreamSubscription[]): void {
    const normalized = normalizeSubscriptions(subscriptions);
    const signature = JSON.stringify(normalized);
    if (signature === this.subscriptionSignature) return;
    this.subscriptions = normalized;
    this.subscriptionSignature = signature;
    this.sendSubscriptions();
  }

  refresh(topics: InstanceStreamTopic[]): void {
    if (this.socket?.readyState !== WebSocket.OPEN) return;
    this.socket.send(JSON.stringify({
      version: 1,
      type: "refresh",
      topics: [...new Set(topics)].sort(),
    }));
  }

  clearTopic(topic: InstanceStreamTopic): void {
    const current = this.snapshot.topics[topic];
    if (!current.data && !current.sampledAt && !current.error) return;
    this.snapshot = {
      ...this.snapshot,
      topics: { ...this.snapshot.topics, [topic]: emptyTopicState() },
    };
    this.emitChange();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    this.listeners.clear();
  }

  private connect(): void {
    if (this.disposed) return;
    this.updateTransport("connecting");
    const socket = new WebSocket(instanceStreamUrl(this.instanceId));
    this.socket = socket;
    socket.addEventListener("open", () => {
      if (this.socket !== socket || this.disposed) return;
      this.reconnectAttempt = 0;
      this.updateTransport("connected");
      this.sendSubscriptions();
    });
    socket.addEventListener("message", (event) => {
      if (this.socket !== socket || this.disposed) return;
      this.receive(event.data);
    });
    socket.addEventListener("close", () => {
      if (this.socket !== socket || this.disposed) return;
      this.socket = null;
      this.updateTransport("disconnected");
      this.scheduleReconnect();
    });
    socket.addEventListener("error", () => {
      if (this.socket === socket && !this.disposed) socket.close();
    });
  }

  private receive(raw: unknown): void {
    if (typeof raw !== "string") return;
    let message: unknown;
    try {
      message = JSON.parse(raw);
    } catch {
      return;
    }
    if (!isRecord(message) || message.version !== 1 || typeof message.type !== "string") return;
    if (message.type === "snapshot" && isTopic(message.topic) && typeof message.sampledAt === "string") {
      this.snapshot = {
        ...this.snapshot,
        topics: {
          ...this.snapshot.topics,
          [message.topic]: { data: message.data ?? null, sampledAt: message.sampledAt, error: null },
        },
      };
      this.emitChange();
      return;
    }
    if (message.type === "channel_error" && isTopic(message.topic) && typeof message.message === "string") {
      const current = this.snapshot.topics[message.topic];
      this.snapshot = {
        ...this.snapshot,
        topics: {
          ...this.snapshot.topics,
          [message.topic]: { ...current, error: message.message },
        },
      };
      this.emitChange();
    }
  }

  private scheduleReconnect(): void {
    if (this.disposed || this.reconnectTimer) return;
    const delay = Math.min(
      reconnectBaseDelayMs * 2 ** this.reconnectAttempt,
      reconnectMaximumDelayMs,
    );
    this.reconnectAttempt += 1;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  private sendSubscriptions(): void {
    if (this.socket?.readyState !== WebSocket.OPEN) return;
    this.socket.send(JSON.stringify({
      version: 1,
      type: "subscribe",
      subscriptions: this.subscriptions,
    }));
  }

  private updateTransport(transportState: InstanceTransportState): void {
    if (this.snapshot.transportState === transportState) return;
    this.snapshot = { ...this.snapshot, transportState };
    this.emitChange();
  }

  private emitChange(): void {
    for (const listener of this.listeners) listener();
  }
}

export function normalizeSubscriptions(
  subscriptions: InstanceStreamSubscription[],
): InstanceStreamSubscription[] {
  const byTopic = new Map<InstanceStreamTopic, InstanceStreamSubscription>();
  for (const subscription of subscriptions) byTopic.set(subscription.topic, subscription);
  return [...byTopic.values()].sort((left, right) =>
    subscriptionKey(left).localeCompare(subscriptionKey(right))
  );
}

export function subscriptionKey(subscription: InstanceStreamSubscription): string {
  return subscription.topic === "runtime.detail"
    ? `${subscription.topic}:${subscription.runId}`
    : subscription.topic;
}

function instanceStreamUrl(instanceId: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/v1/instances/${encodeURIComponent(instanceId)}/stream`;
}

function emptyTopicStates(): Record<InstanceStreamTopic, InstanceStreamTopicState> {
  return {
    overview: emptyTopicState(),
    metrics: emptyTopicState(),
    "runtime.list": emptyTopicState(),
    "runtime.detail": emptyTopicState(),
  };
}

function emptyTopicState(): InstanceStreamTopicState {
  return { data: null, sampledAt: null, error: null };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isTopic(value: unknown): value is InstanceStreamTopic {
  return value === "overview" || value === "metrics" || value === "runtime.list" || value === "runtime.detail";
}
