export interface StateEvidenceData {
  fragment_id: string;
  event_id: string | null;
  evidence_ref_id: string | null;
}

export interface StateViewData {
  state_id: string;
  version_id: string;
  key: string;
  value: unknown;
  valid_from: string;
  valid_to: string | null;
  validity: string;
  evidence: StateEvidenceData[];
  reason?: string | null;
  source_event_id?: string | null;
}

export interface MemoryPacketData {
  query_id: string;
  intent: string;
  current_states: StateViewData[];
  historical_states: StateViewData[];
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
      <StatePacketSections current={packet.current_states} historical={packet.historical_states} />
      <div className="packet-sections">
        {packetSections.map(([key, label]) => {
          const items = packet[key] as unknown[];
          return (
            <section key={key} className={`packet-section ${key === "conflicts" && items.length > 0 ? "packet-conflicts" : ""}`}>
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

function StatePacketSections({ current, historical }: { current: StateViewData[]; historical: StateViewData[] }) {
  const orderedHistory = [...historical].sort(
    (left, right) => Date.parse(right.valid_from) - Date.parse(left.valid_from),
  );
  return (
    <div className="state-packet-sections">
      <section className="state-section">
        <h3>Current States<span>{current.length}</span></h3>
        {current.length === 0 ? <p>当前调用链未提供</p> : (
          <div className="state-table-wrap">
            <table className="state-table" aria-label="Current States">
              <thead><tr><th>State key</th><th>Value</th><th>Validity</th><th>Reason / Evidence</th></tr></thead>
              <tbody>{current.map((state) => (
                <tr key={state.version_id}>
                  <th scope="row"><code>{state.key}</code></th>
                  <td><StateValue value={state.value} /></td>
                  <td><span className="state-validity">{state.validity}</span></td>
                  <td><StateContext state={state} /></td>
                </tr>
              ))}</tbody>
            </table>
          </div>
        )}
      </section>
      <section className="state-section">
        <h3>Historical States<span>{historical.length}</span></h3>
        {orderedHistory.length === 0 ? <p>当前调用链未提供</p> : (
          <ol className="state-timeline" aria-label="Historical States">
            {orderedHistory.map((state) => (
              <li key={state.version_id}>
                <div className="state-timeline-marker" aria-hidden="true" />
                <article>
                  <header><code>{state.key}</code><StateValue value={state.value} /></header>
                  <div className="state-time-range"><time dateTime={state.valid_from}>{formatTimestamp(state.valid_from)}</time><span>-</span><time dateTime={state.valid_to ?? undefined}>{state.valid_to ? formatTimestamp(state.valid_to) : "当前"}</time></div>
                  <StateContext state={state} />
                </article>
              </li>
            ))}
          </ol>
        )}
      </section>
    </div>
  );
}

function StateContext({ state }: { state: StateViewData }) {
  const evidenceIds = state.evidence.flatMap((evidence) => evidence.evidence_ref_id ?? evidence.event_id ?? []);
  if (evidenceIds.length === 0 && state.source_event_id) evidenceIds.push(state.source_event_id);
  return (
    <div className="state-context">
      <span>{state.reason ?? "未提供原因"}</span>
      {evidenceIds.map((id) => <code key={id}>{id}</code>)}
    </div>
  );
}

function StateValue({ value }: { value: unknown }) {
  return <code className="state-value">{typeof value === "string" ? value : JSON.stringify(value)}</code>;
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN", { hour12: false });
}
