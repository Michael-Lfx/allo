import { Component, lazy, Suspense, useMemo, useState, type ComponentType, type ReactNode } from "react";

import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";

export async function retryDynamicImport<T>(load: () => Promise<T>, attempts = 3): Promise<T> {
    let last: unknown;
    for (let attempt = 0; attempt < attempts; attempt += 1) {
        try {
            return await load();
        } catch (error) {
            last = error;
            if (attempt === attempts - 1) throw error;
            await new Promise((resolve) => setTimeout(resolve, 200 * (attempt + 1)));
        }
    }
    throw last;
}

type PropsOf<T> = T extends ComponentType<infer P> ? P : never;

export function CanvasLazyEditor<M extends { default: ComponentType<object> }>({
    load,
    fallback,
    errorTitle,
    ...props
}: {
    load: () => Promise<M>;
    fallback?: ReactNode;
    errorTitle: string;
} & PropsOf<M["default"]>) {
    const [ticket, setTicket] = useState(0);
    const Lazy = useMemo(() => lazy(() => retryDynamicImport(load)), [load, ticket]);
    return (
        <CanvasChunkBoundary title={errorTitle} onRetry={() => setTicket((value) => value + 1)}>
            <Suspense fallback={fallback ?? null}>
                <Lazy {...(props as PropsOf<M["default"]> & object)} />
            </Suspense>
        </CanvasChunkBoundary>
    );
}

class CanvasChunkBoundary extends Component<{ title: string; onRetry: () => void; children: ReactNode }, { error: Error | null }> {
    state = { error: null as Error | null };

    static getDerivedStateFromError(error: Error) {
        return { error };
    }

    render() {
        if (!this.state.error) return this.props.children;
        return (
            <div className="fixed inset-0 z-[var(--z-toast)] grid place-items-center bg-black/45 p-6" role="alert">
                <div className="max-w-md rounded-xl bg-[var(--flowy-panel,Canvas)] p-4 text-sm text-[var(--flowy-text-primary,CanvasText)] shadow-xl">
                    <div className="font-semibold">{this.props.title}加载失败</div>
                    <div className="mt-2 opacity-70">{formatCanvasUserError(this.state.error, "加载失败，请重试")}</div>
                    <button
                        type="button"
                        className="mt-3 rounded-md border px-3 py-1"
                        onClick={() => {
                            this.setState({ error: null });
                            this.props.onRetry();
                        }}
                    >
                        重试
                    </button>
                </div>
            </div>
        );
    }
}
