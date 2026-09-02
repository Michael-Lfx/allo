import { redirect } from "react-router";

/** SPA fallback for the project root `/` and unknown deep links. */
export async function loader() {
  return redirect("/zh-CN");
}

export default function HomeRedirect() {
  return null;
}
