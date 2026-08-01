import { CircleAlert, CircleCheck, Clock3 } from "lucide-react";

interface StatusPillProps {
  state: "ok" | "warning" | "pending";
  children: string;
}

export function StatusPill({ state, children }: StatusPillProps) {
  const Icon = state === "ok" ? CircleCheck : state === "warning" ? CircleAlert : Clock3;
  return (
    <span className={`status-pill status-${state}`}>
      <Icon aria-hidden="true" />
      {children}
    </span>
  );
}
