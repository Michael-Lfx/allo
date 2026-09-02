import { useEffect, useState } from "react";
import { Moon, Sun } from "lucide-react";
import { useTranslation } from "react-i18next";

type Theme = "light" | "dark";

export default function ThemeToggle() {
  const { t } = useTranslation();
  const [theme, setTheme] = useState<Theme>("light");

  useEffect(() => {
    const saved = window.localStorage.getItem("flowy-theme") as Theme | null;
    const initial: Theme =
      saved ?? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
    setTheme(initial);
    document.documentElement.dataset.theme = initial;
  }, []);

  const toggle = () => {
    const next: Theme = theme === "light" ? "dark" : "light";
    setTheme(next);
    document.documentElement.dataset.theme = next;
    window.localStorage.setItem("flowy-theme", next);
  };

  return (
    <button className="icon-btn" onClick={toggle} aria-label={theme === "light" ? t("nav.themeDark") : t("nav.themeLight")}>
      {theme === "light" ? <Moon size={18} /> : <Sun size={18} />}
    </button>
  );
}
