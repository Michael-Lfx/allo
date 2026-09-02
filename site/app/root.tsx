import {
  Links,
  Meta,
  Outlet,
  Scripts,
  ScrollRestoration,
} from "react-router";

import "@fontsource-variable/inter";
import "@fontsource-variable/geist-mono";
import "./styles/style.css";

/**
 * Root document for React Router v8 framework mode (SSG).
 * `Layout` renders the full <html> shell; the default export is the route tree.
 */
export function Layout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="zh-CN">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <Meta />
        <Links />
      </head>
      <body>
        {children}
        <ScrollRestoration />
        <Scripts />
      </body>
    </html>
  );
}

export default function App() {
  return <Outlet />;
}

export const meta = [
  { title: "Flowy Agent Store" },
  {
    name: "description",
    content:
      "本地优先的单文件 Agent 运行时。一个可执行文件内嵌 Web UI，用命令行启动即可在浏览器中管理 Agent、Team、Skill 与 Connector。",
  },
  { property: "og:title", content: "Flowy Agent Store" },
  {
    property: "og:description",
    content:
      "单文件本地优先 Agent 运行时，命令行启动内嵌 Web UI，管理 Agent / Team / Skill / Connector。",
  },
];
