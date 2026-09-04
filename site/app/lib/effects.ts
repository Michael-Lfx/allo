import { useEffect, type CSSProperties } from "react";

/**
 * Landing-page motion helpers — dependency-free, client-only effects.
 * Everything here is decorative; `prefers-reduced-motion` collapses it in CSS.
 */

/** Custom CSS variables are allowed alongside normal style properties. */
type CSSVariables = CSSProperties & Record<`--${string}`, string | number>;

/** Inline style helper: staggered delay for `[data-reveal]` / hero entrance. */
export function revealDelay(ms: number): CSSVariables {
  return { "--reveal-delay": `${ms}ms` };
}

/**
 * Adds `.is-visible` to every `[data-reveal]` element once it enters the
 * viewport (one-shot). Queries the whole document, so mount it ONCE per page —
 * the page component that renders the `[data-reveal]` markup owns this hook.
 */
export function useRevealAll() {
  useEffect(() => {
    const els = Array.from(document.querySelectorAll<HTMLElement>("[data-reveal]"));
    if (!("IntersectionObserver" in window)) {
      for (const el of els) el.classList.add("is-visible");
      return;
    }
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-visible");
            io.unobserve(entry.target);
          }
        }
      },
      { threshold: 0.12, rootMargin: "0px 0px -48px 0px" },
    );
    for (const el of els) io.observe(el);
    return () => io.disconnect();
  }, []);
}

/** Eased integer ramp 0 → target for the stat counters. */
function easeOutQuart(t: number): number {
  return 1 - Math.pow(1 - t, 4);
}

/**
 * Count-up for `[data-count]` elements when they enter the viewport.
 * Attach the hook on the section that owns the markup (same pattern as
 * useRevealAll: one mount observes the whole document).
 */
export function useCountUp() {
  useEffect(() => {
    const els = Array.from(document.querySelectorAll<HTMLElement>("[data-count]"));
    if (!("IntersectionObserver" in window)) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) return; // leave the SSR'd final value in place
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          io.unobserve(entry.target);
          const el = entry.target as HTMLElement;
          const target = el.dataset.count;
          if (!target) continue;
          const final = Number(target);
          if (!Number.isFinite(final)) continue;
          const duration = 900 + Math.min(final, 2000) * 0.15;
          const start = performance.now();
          const tick = (now: number) => {
            const t = Math.min((now - start) / duration, 1);
            el.textContent = String(Math.round(easeOutQuart(t) * final));
            if (t < 1) requestAnimationFrame(tick);
          };
          requestAnimationFrame(tick);
        }
      },
      { threshold: 0.4 },
    );
    for (const el of els) io.observe(el);
    return () => io.disconnect();
  }, []);
}

/**
 * Cursor-follow spotlight: writes `--mx` / `--my` (element-local pixels) on
 * each matched card so CSS can paint a radial highlight under the cursor.
 * Delegated on `container`: cards may mount/unmount (tab switches, filtering)
 * without re-running the effect.
 */
export function useSpotlight(selector: string) {
  useEffect(() => {
    const root = document;
    if (typeof root.addEventListener !== "function") return;
    const onMove = (event: MouseEvent) => {
      const card = (event.target as HTMLElement | null)?.closest<HTMLElement>(selector);
      if (!card) return;
      const rect = card.getBoundingClientRect();
      card.style.setProperty("--mx", `${event.clientX - rect.left}px`);
      card.style.setProperty("--my", `${event.clientY - rect.top}px`);
    };
    root.addEventListener("mousemove", onMove, { passive: true });
    return () => root.removeEventListener("mousemove", onMove);
  }, [selector]);
}

/** Scroll progress: writes `--progress` (0..1) on the `.scroll-progress` bar. */
export function useScrollProgress() {
  useEffect(() => {
    const bar = document.querySelector<HTMLElement>(".scroll-progress");
    if (!bar) return;
    let raf = 0;
    const update = () => {
      raf = 0;
      const doc = document.documentElement;
      const max = doc.scrollHeight - window.innerHeight;
      const progress = max > 0 ? Math.min(window.scrollY / max, 1) : 0;
      bar.style.setProperty("--progress", progress.toFixed(4));
    };
    const schedule = () => {
      if (!raf) raf = requestAnimationFrame(update);
    };
    update();
    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule, { passive: true });
    return () => {
      if (raf) cancelAnimationFrame(raf);
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, []);
}
