import { ChoiceChip, SettingsSection } from "@oc/components/generation-settings-chrome";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import type { CanvasTheme } from "@oc/lib/canvas-theme";

type GenerationCountFieldProps = {
    value: number;
    max: number;
    quickCounts: readonly number[];
    title: string;
    hint?: string;
    ariaLabel: string;
    theme: CanvasTheme;
    onChange: (value: number) => void;
};

export function GenerationCountField({ value, max, quickCounts, title, hint, ariaLabel, theme, onChange }: GenerationCountFieldProps) {
    const chips = quickCounts.filter((item) => item <= max);
    const showCustom = max > Math.max(0, ...chips);
    const customSelected = showCustom && !chips.includes(value);

    return (
        <SettingsSection title={title} hint={hint} theme={theme}>
            <div className="grid grid-cols-4 gap-1.5">
                {chips.map((item) => (
                    <ChoiceChip key={item} selected={value === item} theme={theme} onClick={() => onChange(item)}>
                        {item}
                    </ChoiceChip>
                ))}
                {showCustom ? <CountInput value={value} selected={customSelected} quickCounts={chips} max={max} theme={theme} ariaLabel={ariaLabel} onChange={onChange} /> : null}
            </div>
        </SettingsSection>
    );
}

function CountInput({
    value,
    selected,
    quickCounts,
    max,
    theme,
    ariaLabel,
    onChange,
}: {
    value: number;
    selected: boolean;
    quickCounts: readonly number[];
    max: number;
    theme: CanvasTheme;
    ariaLabel: string;
    onChange: (value: number) => void;
}) {
    const commit = (input: HTMLInputElement) => {
        const next = Math.max(1, Math.min(max, Math.floor(Number(input.value) || 1)));
        input.value = quickCounts.includes(next) ? "" : String(next);
        onChange(next);
    };
    return (
        <label
            className="flex h-8 overflow-hidden rounded-full border text-xs"
            style={{
                background: selected ? theme.toolbar.activeBg : theme.canvas.background,
                borderColor: selected ? theme.node.activeStroke : theme.node.stroke,
                color: theme.node.text,
                boxShadow: selected ? `inset 0 0 0 1px ${theme.node.activeStroke}` : undefined,
            }}
        >
            <input
                key={selected ? `custom-${value}` : "quick"}
                type="number"
                min={1}
                max={max}
                aria-label={ariaLabel}
                placeholder={canvasT("videoCanvas.settings.inputPlaceholder", "输入")}
                className="min-w-0 flex-1 bg-transparent px-2 text-center outline-none placeholder:text-current placeholder:opacity-55 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                style={{ color: theme.node.text, WebkitTextFillColor: theme.node.text }}
                defaultValue={selected ? value : ""}
                onBlur={(event) => commit(event.currentTarget)}
                onKeyDown={(event) => {
                    if (event.key === "Enter") event.currentTarget.blur();
                }}
                onMouseDown={(event) => event.stopPropagation()}
            />
        </label>
    );
}
