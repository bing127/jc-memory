import { Check, Copy } from "lucide-react";
import { useState } from "react";

import { Button } from "./ui/button.js";

export function JsonViewer({ value, label = "Canonical JSON" }: { value: unknown; label?: string }) {
  const [copied, setCopied] = useState(false);
  const json = JSON.stringify(value, null, 2);
  return (
    <section className="json-viewer" aria-label={label}>
      <header><span>{label}</span><Button type="button" variant="ghost" size="icon" title="复制 JSON" onClick={async () => {
        await navigator.clipboard.writeText(json);
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1_500);
      }}>{copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}</Button></header>
      <pre><code>{json}</code></pre>
    </section>
  );
}
