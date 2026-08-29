import { useEffect, useMemo, useRef, useState } from "react";
import { Command, Search } from "lucide-react";
import { Modal } from "antd";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import {
    canvasShortcutCategories,
    canvasShortcuts,
    filterCanvasShortcuts,
    type CanvasShortcutCategoryId,
    type CanvasShortcutItem,
} from "@oc/lib/canvas/canvas-shortcuts";

type ShortcutCategoryFilter = CanvasShortcutCategoryId | "all";

export function CanvasShortcutsModal({ open, onClose }: { open: boolean; onClose: () => void }) {
    useTranslation();
    const inputRef = useRef<HTMLInputElement>(null);
    const [query, setQuery] = useState("");
    const [category, setCategory] = useState<ShortcutCategoryFilter>("all");
    const categories = useMemo(() => canvasShortcutCategories(), []);
    const shortcuts = useMemo(() => canvasShortcuts(), []);

    useEffect(() => {
        if (!open) return;
        setQuery("");
        setCategory("all");
    }, [open]);

    const results = useMemo(() => filterCanvasShortcuts(query, category), [category, query]);
    const categoryCounts = useMemo(
        () => new Map(categories.map((entry) => [entry.id, shortcuts.filter((shortcut) => shortcut.category === entry.id).length])),
        [categories, shortcuts],
    );

    return (
        <Modal
            className="workspace-modal workspace-modal-wide canvas-shortcuts-modal"
            open={open}
            onCancel={onClose}
            footer={null}
            title={null}
            centered
            keyboard
            width="min(860px, calc(100vw - 24px))"
            styles={{ container: { padding: 0 }, body: { padding: 0 } }}
            afterOpenChange={(visible) => {
                if (visible) window.requestAnimationFrame(() => inputRef.current?.focus());
            }}
        >
            <div className="canvas-shortcuts-shell">
                <header className="canvas-shortcuts-header">
                    <div className="canvas-shortcuts-heading">
                        <span className="canvas-shortcuts-heading-icon" aria-hidden>
                            <Command />
                        </span>
                        <span>
                            <strong>{canvasT("videoCanvas.chrome.shortcuts", "快捷键")}</strong>
                            <small>{canvasT("videoCanvas.shortcuts.centerHint", "快速找到键盘、鼠标和视图操作")}</small>
                        </span>
                    </div>
                    <label className="canvas-shortcuts-search">
                        <Search aria-hidden />
                        <input
                            ref={inputRef}
                            value={query}
                            onChange={(event) => setQuery(event.target.value)}
                            placeholder={canvasT("videoCanvas.shortcuts.searchPlaceholder", "搜索按键或操作…")}
                            aria-label={canvasT("videoCanvas.shortcuts.searchAria", "搜索画布快捷键")}
                        />
                        {query ? (
                            <button type="button" onClick={() => setQuery("")} aria-label={canvasT("videoCanvas.shortcuts.clearSearch", "清空搜索")}>
                                {canvasT("videoCanvas.shortcuts.clear", "清除")}
                            </button>
                        ) : null}
                    </label>
                </header>

                <div className="canvas-shortcuts-body">
                    <nav className="canvas-shortcuts-categories" aria-label={canvasT("videoCanvas.shortcuts.categories", "快捷键分类")}>
                        <CategoryButton label={canvasT("videoCanvas.shortcuts.all", "全部")} count={shortcuts.length} active={category === "all"} onClick={() => setCategory("all")} />
                        {categories.map((entry) => (
                            <CategoryButton
                                key={entry.id}
                                label={entry.label}
                                count={categoryCounts.get(entry.id) || 0}
                                active={category === entry.id}
                                onClick={() => setCategory(entry.id)}
                            />
                        ))}
                    </nav>

                    <main className="canvas-shortcuts-results" aria-live="polite">
                        {results.length ? (
                            <div className="canvas-shortcuts-list">
                                {results.map((shortcut) => (
                                    <ShortcutRow key={shortcut.id} shortcut={shortcut} showCategory={category === "all" || Boolean(query)} categories={categories} />
                                ))}
                            </div>
                        ) : (
                            <div className="canvas-shortcuts-empty">
                                <Search aria-hidden />
                                <strong>{canvasT("videoCanvas.shortcuts.emptyTitle", "没有找到相关操作")}</strong>
                                <span>{canvasT("videoCanvas.shortcuts.emptyHint", "试试搜索“粘贴”“缩放”或按键名称")}</span>
                            </div>
                        )}
                    </main>
                </div>

                <footer className="canvas-shortcuts-footer">
                    <span>{canvasT("videoCanvas.shortcuts.count", "共 {{n}} 个快捷键", { n: results.length })}</span>
                    <span className="canvas-shortcuts-close-hint"><kbd>Esc</kbd> {canvasT("videoCanvas.shortcuts.close", "关闭")}</span>
                </footer>
            </div>
        </Modal>
    );
}

function CategoryButton({ label, count, active, onClick }: { label: string; count: number; active: boolean; onClick: () => void }) {
    return (
        <button type="button" className={active ? "is-active" : undefined} aria-pressed={active} onClick={onClick}>
            <span>{label}</span>
            <small>{count}</small>
        </button>
    );
}

function ShortcutRow({ shortcut, showCategory, categories }: { shortcut: CanvasShortcutItem; showCategory: boolean; categories: ReturnType<typeof canvasShortcutCategories> }) {
    const categoryLabel = categories.find((entry) => entry.id === shortcut.category)?.label;

    return (
        <article className="canvas-shortcuts-row">
            <div className="canvas-shortcuts-row-copy">
                <span className="canvas-shortcuts-row-title">
                    <strong>{shortcut.title}</strong>
                    {showCategory ? <small>{categoryLabel}</small> : null}
                </span>
                <p>{shortcut.description}</p>
            </div>
            <div className="canvas-shortcuts-keys" aria-label={shortcut.keys.map((combination) => combination.join(" + ")).join(" / ")}>
                {shortcut.keys.map((combination, combinationIndex) => (
                    <span key={`${shortcut.id}-${combination.join("-")}`} className="canvas-shortcuts-combination">
                        {combinationIndex ? <em>{canvasT("videoCanvas.shortcuts.or", "或")}</em> : null}
                        {combination.map((key, keyIndex) => (
                            <span key={`${key}-${keyIndex}`} className="canvas-shortcuts-key-part">
                                {keyIndex ? <i>+</i> : null}
                                <kbd>{key}</kbd>
                            </span>
                        ))}
                    </span>
                ))}
            </div>
        </article>
    );
}
