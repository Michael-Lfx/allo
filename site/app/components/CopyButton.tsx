import { useState } from "react";
import { Check, Copy } from "lucide-react";

export default function CopyButton({
  text,
  label = "Copy",
  tone = "light",
}: {
  text: string;
  label?: string;
  tone?: "light" | "dark";
}) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      console.error("clipboard copy failed");
    }
  };
  return (
    <button
      className={tone === "dark" ? "copy-btn copy-btn-dark" : "copy-btn"}
      onClick={copy}
      aria-label={label}
      type="button"
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </button>
  );
}
