import { zodResolver } from "@hookform/resolvers/zod";
import { ArrowLeft, ArrowRight, Boxes, LockKeyhole, ShieldCheck } from "lucide-react";
import { useForm } from "react-hook-form";
import { Link } from "react-router-dom";
import { z } from "zod";

import { Button } from "../components/ui/button.js";
import { Input } from "../components/ui/input.js";

const formSchema = z.object({ password: z.string().min(1, "请输入管理员密码").max(256) });
type LoginForm = z.infer<typeof formSchema>;

interface LoginPageProps {
  pending: boolean;
  error: string | null;
  onLogin(password: string): Promise<void>;
}

export function LoginPage({ pending, error, onLogin }: LoginPageProps) {
  const form = useForm<LoginForm>({
    resolver: zodResolver(formSchema),
    defaultValues: { password: "" },
  });

  return (
    <main className="relative grid min-h-screen place-items-center overflow-hidden bg-[#080b0a] px-5 py-20 text-white">
      <img className="absolute inset-0 h-full w-full object-cover opacity-20" src="/control-plane-preview.png" alt="" />
      <div className="absolute inset-0 bg-black/72" aria-hidden="true" />
      <Link className="absolute left-5 top-5 z-10 inline-flex h-10 items-center gap-2 border border-white/12 bg-black/50 px-3 text-xs text-white/65 hover:text-white md:left-8 md:top-8" to="/">
        <ArrowLeft size={15} aria-hidden="true" />返回首页
      </Link>
      <section className="relative z-10 grid w-full max-w-4xl overflow-hidden border border-white/12 bg-[#0e1211]/95 shadow-2xl shadow-black/50 md:grid-cols-[0.9fr_1.1fr]" aria-labelledby="login-title">
        <div className="hidden border-r border-white/10 p-9 md:flex md:flex-col">
          <div className="flex items-center gap-3"><span className="grid size-9 place-items-center bg-emerald-300 text-black"><Boxes size={18} aria-hidden="true" /></span><strong>疆诚记忆</strong></div>
          <div className="mt-auto">
            <p className="text-xs font-semibold text-emerald-300">CONTROL PLANE</p>
            <h1 className="mt-4 text-4xl font-semibold leading-tight">一条可追溯的<br />Agent 运行链路。</h1>
            <div className="mt-10 grid gap-3 text-xs text-white/55">
              <span className="flex items-center gap-2"><LockKeyhole size={14} className="text-emerald-300" />实例凭据不进入浏览器</span>
              <span className="flex items-center gap-2"><ShieldCheck size={14} className="text-emerald-300" />Scope 与授权由服务端确定</span>
            </div>
          </div>
        </div>
        <div className="bg-[#f7f8f7] p-7 text-[#111513] sm:p-10 md:p-12">
          <p className="page-kicker">LOCAL ADMIN SESSION</p>
          <h2 className="mt-3 text-3xl font-semibold" id="login-title">进入控制台</h2>
          <p className="login-description">使用部署时配置的管理员凭据建立安全会话。</p>
          <form
            onSubmit={form.handleSubmit(async ({ password }) => onLogin(password))}
            className="login-form"
          >
            <label htmlFor="admin-password">管理员密码</label>
            <Input
              id="admin-password"
              type="password"
              autoComplete="current-password"
              aria-invalid={Boolean(form.formState.errors.password)}
              {...form.register("password")}
            />
            {form.formState.errors.password ? (
              <p className="form-error" role="alert">{form.formState.errors.password.message}</p>
            ) : null}
            {error ? <p className="form-error" role="alert">{error}</p> : null}
            <Button type="submit" disabled={pending} className="mt-2 w-full">
              {pending ? "正在验证" : "进入控制台"}
              <ArrowRight aria-hidden="true" size={16} />
            </Button>
          </form>
          <p className="login-footnote">Session Cookie 使用 HttpOnly 与 SameSite=Strict；实例凭据不会进入浏览器。</p>
        </div>
      </section>
    </main>
  );
}
