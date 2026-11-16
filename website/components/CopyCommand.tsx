"use client";

import { useState } from "react";
import { LuCheck, LuCopy } from "react-icons/lu";

export default function CopyCommand({ command, label = "Install" }: { command: string; label?: string }) {
  const [status, setStatus] = useState("");

  async function copy() {
    try {
      await navigator.clipboard.writeText(command);
      setStatus("Copied");
    } catch {
      setStatus("Select the text to copy");
    }
    setTimeout(() => setStatus(""), 1800);
  }

  return (
    <div className="copy">
      <span className="copy-prompt" aria-hidden="true">$</span>
      <code className="copy-text" aria-label={`${label} command`}>{command}</code>
      <button type="button" className="copy-button" onClick={copy}>
        {status === "Copied" ? <LuCheck aria-hidden="true" /> : <LuCopy aria-hidden="true" />}
        <span className="sr-only"> the {label.toLowerCase()} command</span>
      </button>
      <span className="sr-only" role="status" aria-live="polite">
        {status}
      </span>
    </div>
  );
}
