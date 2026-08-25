/** 调色参数，单位与 CSS filter 一致（百分比 / 度） */
export type CanvasColorGrade = {
    brightness: number;
    contrast: number;
    saturate: number;
    hueRotate: number;
};

export const DEFAULT_COLOR_GRADE: CanvasColorGrade = { brightness: 100, contrast: 100, saturate: 100, hueRotate: 0 };

export function isNeutralColorGrade(grade: CanvasColorGrade) {
    return grade.brightness === 100 && grade.contrast === 100 && grade.saturate === 100 && grade.hueRotate === 0;
}

/**
 * 预览（img 的 CSS filter）共用字符串。
 * 桌面端仅做本地预览；不接云端上传落地（open-ai 的 resolveCanvasColorGradeReference 已跳过）。
 */
export function colorGradeCssFilter(grade: CanvasColorGrade) {
    return `brightness(${grade.brightness}%) contrast(${grade.contrast}%) saturate(${grade.saturate}%) hue-rotate(${grade.hueRotate}deg)`;
}
