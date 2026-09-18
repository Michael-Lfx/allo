import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./i18n";
import "@fontsource-variable/inter";
import "@fontsource-variable/noto-sans-sc";
import "@fontsource-variable/geist-mono";
import "./style.css";
import { applyInitialTheme } from "./ui/theme";

applyInitialTheme();

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root element");
}
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);