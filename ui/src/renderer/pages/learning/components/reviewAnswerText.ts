import type { ActivityKind, NumericUnit } from '../types';

/** 数值揭示格式（w2）：precision = 显示小数位；unit = percent 时加 %。
 * 只影响展示，判分与提交值仍按后端 number 契约。 */
export interface NumericRevealFormat {
  precision?: number | null;
  unit?: NumericUnit | null;
}

/** 标量答案 → 文本：字符串原样、数值/布尔 String()、数组递归、对象逐对；
 * 未知形状走 JSON.stringify 可见降级，绝不返回空串吞掉答案（x03）。 */
const scalarText = (value: unknown): string => {
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (Array.isArray(value)) return value.map(scalarText).filter((item) => item !== '').join(' / ');
  if (value !== null && typeof value === 'object') {
    return Object.entries(value as Record<string, unknown>)
      .map(([key, item]) => `${key} → ${scalarText(item)}`)
      .join('；');
  }
  return '';
};

/** 数值答案按 precision/unit 格式化（w2）：未声明 precision 时不补零，
 * percent 只追加 % 符号（不是文案，无需 i18n key）。 */
const numericText = (value: number, format?: NumericRevealFormat): string => {
  const precision =
    typeof format?.precision === 'number' ? Math.min(Math.max(Math.trunc(format.precision), 0), 10) : null;
  const text = precision === null ? String(value) : value.toFixed(precision);
  return format?.unit === 'percent' ? `${text}%` : text;
};

/**
 * 揭示正确答案的可读文本。答案形状由题型隐式决定（后端 `validate_shape`），
 * 前端按 `kind` 分派：matching 按左列逐对、ordering 按序号列出；numeric 按
 * precision/unit 格式化（w2）；未知形状兜底为 JSON，不静默变空串。
 */
export const answerText = (
  value: unknown,
  kind: ActivityKind,
  options: readonly string[],
  translate: (key: string) => string,
  format?: NumericRevealFormat
): string => {
  if (value === null || value === undefined) return '';
  if (kind === 'true_false' && typeof value === 'boolean') {
    return value ? translate('learning.trueLabel') : translate('learning.falseLabel');
  }
  if (kind === 'numeric' && typeof value === 'number') {
    return numericText(value, format);
  }
  if (kind === 'matching' && Array.isArray(value)) {
    return value
      .map((right, index) => `${options[index] ?? `#${index + 1}`} → ${scalarText(right)}`)
      .join('；');
  }
  if (kind === 'ordering' && Array.isArray(value)) {
    return value.map((item, index) => `${index + 1}. ${scalarText(item)}`).join('；');
  }
  const text = scalarText(value);
  if (text !== '') return text;
  // 未知形状：可见降级，不吞空。
  return JSON.stringify(value) ?? String(value);
};
