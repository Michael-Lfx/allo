import { memo, useRef, useState, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { useTranslation } from "react-i18next";
import "highlight.js/styles/github.css";

/**
 * Agent replies arrive as a Markdown string (already normalized by
 * `contentToText`). We render it with react-markdown and keep the safe default
 * of NOT parsing raw HTML — model/agent output can contain untrusted content,
 * so no `rehype-raw`. Links open in a new tab, code blocks get a language label
 * and a copy button.
 */

type ChildWithProps = ReactNode & { props?: { className?: string } };

function codeChild(children: ReactNode): ChildWithProps | undefined {
  if (Array.isArray(children)) return children[0] as ChildWithProps | undefined;
  return children as ChildWithProps | undefined;
}

function CodeBlock({ children }: { children?: ReactNode }) {
  const { t } = useTranslation();
  const preRef = useRef<HTMLPreElement | null>(null);
  const [copied, setCopied] = useState(false);

  const child = codeChild(children);
  const langClass = child?.props?.className ?? "";
  const lang = /language-([\w-]+)/.exec(langClass)?.[1] ?? "";

  const onCopy = async () => {
    const text = preRef.current?.textContent ?? "";
    if (!text || !navigator.clipboard) return;
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="code-block">
      <div className="code-block-head">
        <span className="code-lang">{lang}</span>
        <button type="button" className="code-copy" onClick={() => void onCopy()}>
          {copied ? t("message.copyCodeDone") : t("message.copyCode")}
        </button>
      </div>
      <pre ref={preRef}>{children}</pre>
    </div>
  );
}

const components: Components = {
  a: ({ node: _node, ...props }) => (
    <a target="_blank" rel="noreferrer noopener" {...props} />
  ),
  img: ({ node: _node, ...props }) => (
    <img referrerPolicy="no-referrer" {...props} />
  ),
  pre: ({ node: _node, children }) => <CodeBlock>{children}</CodeBlock>,
};

export const Markdown = memo(function Markdown({ source }: { source: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[rehypeHighlight]}
      components={components}
    >
      {source}
    </ReactMarkdown>
  );
});
