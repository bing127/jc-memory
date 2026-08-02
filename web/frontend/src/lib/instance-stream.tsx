import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import type {
  InstanceStreamSubscription,
  InstanceStreamTopic,
  InstanceStreamTopicState,
  InstanceTransportState,
} from "./contracts.js";
import {
  acquireInstanceStreamClient,
  normalizeSubscriptions,
  releaseInstanceStreamClient,
  subscriptionKey,
  type InstanceStreamClient,
} from "./instance-stream-client.js";

interface InstanceStreamContextValue {
  setSubscription(token: symbol, subscription: InstanceStreamSubscription): void;
  removeSubscription(token: symbol): void;
  topicStates: Record<InstanceStreamTopic, InstanceStreamTopicState>;
  transportState: InstanceTransportState;
  refresh(topics: InstanceStreamTopic[]): void;
}

const emptyState: InstanceStreamTopicState = { data: null, sampledAt: null, error: null };
const InstanceStreamContext = createContext<InstanceStreamContextValue | null>(null);

export function InstanceStreamProvider({
  instanceId,
  children,
}: {
  instanceId: string | null;
  children: ReactNode;
}) {
  const clientRef = useRef<InstanceStreamClient | null>(null);
  const subscriptionsRef = useRef(new Map<symbol, InstanceStreamSubscription>());
  const flushScheduledRef = useRef(false);
  const lastRuntimeDetailKeyRef = useRef<string | null>(null);
  const [snapshot, setSnapshot] = useState(() => disconnectedSnapshot());

  const flushSubscriptions = useCallback(() => {
    flushScheduledRef.current = false;
    const subscriptions = normalizeSubscriptions([...subscriptionsRef.current.values()]);
    clientRef.current?.setSubscriptions(subscriptions);
  }, []);

  const scheduleSubscriptionFlush = useCallback(() => {
    if (flushScheduledRef.current) return;
    flushScheduledRef.current = true;
    queueMicrotask(flushSubscriptions);
  }, [flushSubscriptions]);

  const setSubscription = useCallback((token: symbol, subscription: InstanceStreamSubscription) => {
    subscriptionsRef.current.set(token, subscription);
    if (subscription.topic === "runtime.detail") {
      const nextKey = subscriptionKey(subscription);
      if (lastRuntimeDetailKeyRef.current && lastRuntimeDetailKeyRef.current !== nextKey) {
        clientRef.current?.clearTopic("runtime.detail");
      }
      lastRuntimeDetailKeyRef.current = nextKey;
    }
    scheduleSubscriptionFlush();
  }, [scheduleSubscriptionFlush]);

  const removeSubscription = useCallback((token: symbol) => {
    subscriptionsRef.current.delete(token);
    scheduleSubscriptionFlush();
  }, [scheduleSubscriptionFlush]);

  const refresh = useCallback((topics: InstanceStreamTopic[]) => {
    clientRef.current?.refresh(topics);
  }, []);

  useEffect(() => {
    if (!instanceId) {
      clientRef.current = null;
      setSnapshot(disconnectedSnapshot());
      return;
    }
    const client = acquireInstanceStreamClient(instanceId);
    clientRef.current = client;
    setSnapshot(client.getSnapshot());
    const unsubscribe = client.subscribe(() => setSnapshot(client.getSnapshot()));
    scheduleSubscriptionFlush();
    return () => {
      unsubscribe();
      if (clientRef.current === client) clientRef.current = null;
      releaseInstanceStreamClient(instanceId, client);
    };
  }, [instanceId, scheduleSubscriptionFlush]);

  const value = useMemo<InstanceStreamContextValue>(() => ({
    setSubscription,
    removeSubscription,
    topicStates: snapshot.topics,
    transportState: snapshot.transportState,
    refresh,
  }), [refresh, removeSubscription, setSubscription, snapshot]);

  return <InstanceStreamContext.Provider value={value}>{children}</InstanceStreamContext.Provider>;
}

export function useInstanceStreamSubscription(subscription: InstanceStreamSubscription | null): void {
  const { setSubscription, removeSubscription } = useRequiredContext();
  const tokenRef = useRef(Symbol("instance-stream-subscription"));
  const key = subscription ? subscriptionKey(subscription) : "";
  useEffect(() => {
    if (!subscription) return;
    const token = tokenRef.current;
    setSubscription(token, subscription);
    return () => removeSubscription(token);
  }, [key, removeSubscription, setSubscription]);
}

export function useInstanceStreamTopic<T>(topic: InstanceStreamTopic): InstanceStreamTopicState<T> {
  const state = useRequiredContext().topicStates[topic];
  return state as InstanceStreamTopicState<T>;
}

export function useInstanceTransportState(): InstanceTransportState {
  return useRequiredContext().transportState;
}

export function useInstanceStreamRefresh(): (topics: InstanceStreamTopic[]) => void {
  return useRequiredContext().refresh;
}

function useRequiredContext(): InstanceStreamContextValue {
  const context = useContext(InstanceStreamContext);
  if (!context) throw new Error("实例订阅 Hook 必须在 InstanceStreamProvider 内使用");
  return context;
}

function disconnectedSnapshot(): {
  transportState: InstanceTransportState;
  topics: Record<InstanceStreamTopic, InstanceStreamTopicState>;
} {
  return {
    transportState: "disconnected",
    topics: {
      overview: { ...emptyState },
      metrics: { ...emptyState },
      "runtime.list": { ...emptyState },
      "runtime.detail": { ...emptyState },
    },
  };
}
