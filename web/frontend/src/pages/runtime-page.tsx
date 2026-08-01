import { useMutation } from "@tanstack/react-query";
import { ChevronRight, Clock3, GitBranch, PauseCircle, Play } from "lucide-react";
import { useState } from "react";
import { useSearchParams } from "react-router-dom";

import { JsonViewer } from "../components/json-viewer.js";
import { Button } from "../components/ui/button.js";
import { api, ApiError } from "../lib/api.js";
import type {
  InstanceSummary,
  RuntimeRunDetail,
  RuntimeRunPage,
  RuntimeRunRecord,
  RuntimeState,
} from "../lib/contracts.js";
import {
  useInstanceStreamRefresh,
  useInstanceStreamSubscription,
  useInstanceStreamTopic,
} from "../lib/instance-stream.js";

export function RuntimePage({ instance }: { instance: InstanceSummary }) {
  const [searchParams, setSearchParams] = useSearchParams();
  const [prompt, setPrompt] = useState("");
  const selectedRunId = searchParams.get("run");
  useInstanceStreamSubscription({ topic: "runtime.list" });
  useInstanceStreamSubscription(
    selectedRunId ? { topic: "runtime.detail", runId: selectedRunId } : null,
  );
  const runs = useInstanceStreamTopic<RuntimeRunPage>("runtime.list");
  const detail = useInstanceStreamTopic<RuntimeRunDetail>("runtime.detail");
  const refresh = useInstanceStreamRefresh();
  const start = useMutation({
    mutationFn: () => api.runtimeStart(instance.id, prompt),
    onSuccess(result) {
      setSearchParams({ run: result.run_id });
      refresh(["runtime.list"]);
    },
  });

  return (
    <main className="console-page">
      <header className="page-header">
        <div><p className="page-kicker">RUNTIME EXPLORER</p><h1>Agent Runtime</h1></div>
        <p>启动 FinalResponse Run，并查看持久化的 Transition 与 Control Checkpoint。</p>
      </header>
      <form
        className="runtime-start"
        onSubmit={(event) => {
          event.preventDefault();
          if (prompt.trim()) start.mutate();
        }}
      >
        <label htmlFor="runtime-prompt">Runtime Prompt</label>
        <textarea id="runtime-prompt" value={prompt} maxLength={32_768} onChange={(event) => setPrompt(event.target.value)} />
        <Button type="submit" disabled={start.isPending || !prompt.trim()}>
          <Play aria-hidden="true" size={15} />{start.isPending ? "正在启动" : "启动 Run"}
        </Button>
        {start.data?.response ? <output className="runtime-start-result">{start.data.response.text}</output> : null}
        {start.error instanceof ApiError && start.error.status === 503 ? <span className="runtime-start-error">当前实例未配置 Runtime Provider</span> : null}
        {start.isError && !(start.error instanceof ApiError && start.error.status === 503) ? <span className="runtime-start-error">Runtime 启动失败</span> : null}
      </form>
      <div
        className={`error-band ${runs.error ? "visible" : "invisible"}`}
        role="alert"
        aria-hidden={!runs.error}
      >
        {runs.error ? "Runtime 查询失败，请检查实例版本、API Key 与 Scope。" : "Runtime 列表频道正常"}
      </div>
      <div className="runtime-layout">
          <section className="runtime-list" aria-label="Runtime Runs">
            <header><div><span>RUNS</span><strong>{runs.data?.total ?? 0}</strong></div><small>实时订阅</small></header>
            {!runs.data && !runs.error ? <p className="runtime-empty">正在读取持久化 Run</p> : null}
            {runs.data?.items.length === 0 ? <p className="runtime-empty">当前 Scope 暂无 Runtime Run</p> : null}
            {runs.data?.items.map((run) => (
              <RunRow
                key={run.run_id}
                run={run}
                selected={selectedRunId === run.run_id}
                onSelect={() => setSearchParams({ run: run.run_id })}
              />
            ))}
          </section>
          <section className="runtime-detail" aria-label="Runtime Run 详情">
            {!selectedRunId ? <div className="runtime-detail-empty"><GitBranch aria-hidden="true" /><span>选择 Run 查看不可变 Transition 时间线</span></div> : null}
            {!detail.data && !detail.error && selectedRunId ? <div className="runtime-detail-empty">正在读取 Run 详情</div> : null}
            {detail.error ? <div className="error-band">无法读取该 Run，可能已超出当前授权 Scope。</div> : null}
            {detail.data ? (
              <>
                <div className="runtime-detail-header">
                  <div><span>RUN ID</span><code>{detail.data.run.run_id}</code></div>
                  <StateBadge state={detail.data.run.state} />
                </div>
                <div className="runtime-facts">
                  <span>Next step <strong>{detail.data.run.next_step}</strong></span>
                  <span>Version <strong>{detail.data.run.version}</strong></span>
                  <span>Updated <strong>{formatMicros(detail.data.run.updated_at_us)}</strong></span>
                  <span>Control <strong>{detail.data.run.control?.state ?? "当前调用链未提供"}</strong></span>
                </div>
                {detail.data.run.control ? (
                  <div className="checkpoint-strip"><PauseCircle aria-hidden="true" /><span>{detail.data.run.control.state}</span><code>resume: {detail.data.run.control.resume_state}</code><code>attempt: {detail.data.run.control.attempt}</code></div>
                ) : null}
                <div className="transition-timeline">
                  {detail.data.transitions.map((transition) => (
                    <article className="transition-row" key={transition.id}>
                      <div className="transition-step">{transition.step}</div>
                      <div className="transition-body">
                        <header><strong>{transition.from_state} -&gt; {transition.to_state}</strong><span>attempt {transition.attempt}</span></header>
                        <div className="transition-meta"><code>{transition.trace_id}</code><span>{formatMicros(transition.created_at_us)}</span></div>
                        <pre>{JSON.stringify(transition.payload, null, 2)}</pre>
                        <div className="transition-evidence"><span>Input refs {transition.input_refs.length}</span><span>Output refs {transition.output_refs.length}</span></div>
                      </div>
                    </article>
                  ))}
                </div>
                <JsonViewer value={detail.data} label="Runtime Canonical JSON" />
              </>
            ) : null}
          </section>
        </div>
    </main>
  );
}

function RunRow({ run, selected, onSelect }: { run: RuntimeRunRecord; selected: boolean; onSelect(): void }) {
  return (
    <div className={`runtime-run-row ${selected ? "is-selected" : ""}`}>
      <button type="button" aria-label="查看 Run" onClick={onSelect}>
        <div><code>{run.run_id}</code><span><Clock3 aria-hidden="true" />{formatMicros(run.updated_at_us)}</span></div>
        <StateBadge state={run.state} />
        <ChevronRight aria-hidden="true" />
      </button>
    </div>
  );
}

function StateBadge({ state }: { state: RuntimeState }) {
  const tone = state === "completed" ? "ok" : state === "failed" || state === "cancelled" ? "danger" : "active";
  return <span className={`runtime-state runtime-state-${tone}`}>{state}</span>;
}

function formatMicros(value: number): string {
  return new Date(Math.trunc(value / 1_000)).toLocaleString("zh-CN", { hour12: false });
}
