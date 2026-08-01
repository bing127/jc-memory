export interface MemoryPacketData {
  query_id: string;
  intent: string;
  current_states: unknown[];
  historical_states: unknown[];
  activated_surfaces: unknown[];
  episodes: unknown[];
  points: unknown[];
  paths: unknown[];
  procedures: unknown[];
  constraints: unknown[];
  preferences: unknown[];
  past_failures: unknown[];
  evidence: unknown[];
  conflicts: unknown[];
  missing_evidence: unknown[];
  coverage: {
    seed_count: number;
    visited_nodes: number;
    max_depth: number;
    evidence_count: number;
    missing_evidence_count: number;
    truncated: boolean;
  };
  confidence: number;
  recommended_context: string;
}

const packetSections: Array<[keyof MemoryPacketData, string]> = [
  ["current_states", "Current States"],
  ["historical_states", "Historical States"],
  ["activated_surfaces", "Activated Surfaces"],
  ["episodes", "Episodes"],
  ["points", "Points"],
  ["paths", "Paths"],
  ["procedures", "Procedures"],
  ["constraints", "Constraints"],
  ["preferences", "Preferences"],
  ["past_failures", "Past Failures"],
  ["evidence", "Evidence"],
  ["conflicts", "Conflicts"],
  ["missing_evidence", "Missing Evidence"],
];

export function MemoryPacket({ packet }: { packet: MemoryPacketData }) {
  return (
    <section className="packet-view" aria-label="Memory Packet">
      <header className="packet-header">
        <div><span>QUERY ID</span><code>{packet.query_id}</code></div>
        <div><span>INTENT</span><strong>{packet.intent}</strong></div>
        <div><span>CONFIDENCE</span><strong>{packet.confidence.toFixed(3)}</strong></div>
      </header>
      <div className="coverage-strip">
        <span>seeds: {packet.coverage.seed_count}</span>
        <span>nodes: {packet.coverage.visited_nodes}</span>
        <span>depth: {packet.coverage.max_depth}</span>
        <span>evidence: {packet.coverage.evidence_count}</span>
        <span>truncated: {String(packet.coverage.truncated)}</span>
      </div>
      <div className="packet-sections">
        {packetSections.map(([key, label]) => {
          const items = packet[key] as unknown[];
          return (
            <section key={key} className="packet-section">
              <h3>{label}<span>{items.length}</span></h3>
              {items.length === 0 ? (
                <p>当前调用链未提供</p>
              ) : (
                <pre>{JSON.stringify(items, null, 2)}</pre>
              )}
            </section>
          );
        })}
      </div>
    </section>
  );
}
