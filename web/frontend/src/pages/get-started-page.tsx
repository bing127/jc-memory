import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, Cloud, DatabaseZap, ShieldCheck } from "lucide-react";
import { useForm } from "react-hook-form";
import { useNavigate } from "react-router-dom";
import { z } from "zod";

import { Button } from "../components/ui/button.js";
import { Input } from "../components/ui/input.js";
import { api, ApiError } from "../lib/api.js";
import type { InstanceSummary } from "../lib/contracts.js";

const schema = z.object({
  name: z.string().trim().min(1, "请输入实例名称").max(80),
  baseUrl: z.url("请输入有效 URL"),
  namespaceId: z.string().regex(/^[0-9a-f-]{36}$/i, "请输入 Namespace UUID"),
  apiKey: z
    .string()
    .regex(
      /^sk-jcmemory-[a-z0-9]{34}$/,
      "API Key 必须以 sk-jcmemory- 开头，并包含 34 位小写字母或数字",
    ),
});
type FormValues = z.infer<typeof schema>;

export function GetStartedPage({ instances }: { instances: InstanceSummary[] }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { name: "", baseUrl: "http://127.0.0.1:8787", namespaceId: "", apiKey: "" },
  });
  const create = useMutation({
    mutationFn: api.createInstance,
    async onSuccess(instance) {
      await queryClient.invalidateQueries({ queryKey: ["instances"] });
      navigate(`/instances/${instance.id}/overview`);
    },
  });
  return (
    <main className="console-page">
      <header className="page-header">
        <div><p className="page-kicker">GET STARTED</p><h1>连接记忆实例</h1></div>
        <p>连接现有 Server 或 Sidecar；删除连接不会删除实例中的任何记忆。</p>
      </header>

      <section className="connection-layout">
        <form
          className="connection-form"
          onSubmit={form.handleSubmit(async (values) => create.mutateAsync(values))}
        >
          <div className="section-heading"><DatabaseZap aria-hidden="true" /><div><h2>Self-hosted Instance</h2><p>LIVE · REST v1</p></div></div>
          <Field label="实例名称" error={form.formState.errors.name?.message}><Input {...form.register("name")} /></Field>
          <Field label="Server URL" error={form.formState.errors.baseUrl?.message}><Input inputMode="url" {...form.register("baseUrl")} /></Field>
          <Field label="Namespace ID" error={form.formState.errors.namespaceId?.message}><Input {...form.register("namespaceId")} /></Field>
          <Field label="API Key" error={form.formState.errors.apiKey?.message}><Input type="password" autoComplete="off" placeholder="sk-jcmemory-..." {...form.register("apiKey")} /></Field>
          {create.error ? <p className="form-error" role="alert">{create.error instanceof ApiError ? create.error.message : "连接保存失败"}</p> : null}
          <Button type="submit" disabled={create.isPending}>保存并检查连接<ArrowRight aria-hidden="true" size={16} /></Button>
          <div className="boundary-list" aria-label="连接边界">
            <span><ShieldCheck aria-hidden="true" /> API Key 仅由后端加密保存</span>
          </div>
        </form>

        <div className="connection-side">
          <section className="platform-band">
            <div className="section-heading"><Cloud aria-hidden="true" /><div><h2>Managed Sandbox</h2><p>PLATFORM · 尚未实现</p></div></div>
            <p>仓库当前没有 Sandbox Provision 与生命周期 API，因此这里不提供虚假的创建操作。</p>
          </section>
          {instances.length > 0 ? (
            <section className="instance-list">
              <h2>已连接实例</h2>
              {instances.map((instance) => (
                <button key={instance.id} type="button" onClick={() => navigate(`/instances/${instance.id}/overview`)}>
                  <span><strong>{instance.name}</strong><small>{instance.baseUrl}</small></span>
                  <ArrowRight aria-hidden="true" size={16} />
                </button>
              ))}
            </section>
          ) : null}
        </div>
      </section>
    </main>
  );
}

function Field({ label, error, children }: { label: string; error: string | undefined; children: React.ReactNode }) {
  return <label className="form-field"><span>{label}</span>{children}{error ? <small className="form-error">{error}</small> : null}</label>;
}
