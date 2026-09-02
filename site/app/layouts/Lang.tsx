import { Outlet, useParams } from "react-router";

import { applyLanguage, type Language } from "../i18n";
import NavBar from "../components/NavBar";
import Footer from "../components/Footer";

export async function loader({ params }: { params: { lang?: string } }) {
  applyLanguage(params.lang === "en-US" ? "en-US" : "zh-CN");
  return null;
}

function useLang(): Language {
  const { lang } = useParams();
  return lang === "en-US" ? "en-US" : "zh-CN";
}

export default function LangLayout() {
  const lang = useLang();
  return (
    <div className="site">
      <NavBar lang={lang} />
      <main className="site-main" id="main">
        <Outlet />
      </main>
      <Footer lang={lang} />
    </div>
  );
}
