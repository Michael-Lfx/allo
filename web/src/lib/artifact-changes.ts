import { normalizeArtifactKey } from "./artifact-owners";

/**
 * R20b：工作区「变更 → 接受 / 回退」的纯层。
 *
 * 后端原语**已存在**：`nomifun-file` 的 git 基线快照服务，经宿主面文件服务暴露
 * （`POST /api/fs/snapshot/{init,compare,stage,stage-all,unstage,discard}`）。前端
 * 只做封装 + 展示，不引入 Artifact 协议（`artifact/list` / `artifact/get` 仍延后，
 * `05` §8 / TC-AS-008）：
 *   - `compare` 给出**待处理（unstaged）**与**已接受（staged）**两组变更；
 *   - 接受 = `stage`（把这次改动登记进基线索引，文件内容不变）；
 *   - 回退 = `discard`（`create` 删掉新文件，`modify` / `delete` 从基线恢复）。
 *
 * 这里只放**纯函数**（归一化 / 排序 / 计数）：不发请求、不碰 React、不碰 i18n。
 */

/** `nomifun_common::FileChangeOperation`（serde `lowercase`）。 */
export type FileChangeOperation = "create" | "modify" | "delete";

/** `nomifun_api_types::SnapshotMode`（serde `kebab-case`）。 */
export type SnapshotMode = "git-repo" | "snapshot" | "disabled";

/** 一条相对基线的文件变更（`POST /api/fs/snapshot/compare`）。 */
export interface FileChangeInfo {
  /** 绝对路径。 */
  file_path: string;
  /**
   * 相对工作区根的路径——**回传给 `stage` / `unstage` / `discard` 的就是它**
   * （服务端用 `workdir.join(relative_path)` 定位文件，靠 `file_path` 会拼错）。
   */
  relative_path: string;
  operation: FileChangeOperation;
}

/** `compare` 的结果：待处理与已接受两组。 */
export interface SnapshotCompare {
  staged: FileChangeInfo[];
  unstaged: FileChangeInfo[];
}

/**
 * `POST /api/fs/snapshot/init` 的结果。`mode === "disabled"` 时 `reason` 说明
 * 为什么拒绝跟踪（盘符根 / 系统目录 / 过大等），此时没有可审查的变更。
 */
export interface SnapshotInfo {
  mode: SnapshotMode;
  branch: string | null;
  reason: string | null;
}

/** 稳定的空 compare（引用不变，避免选择器每次返回新对象触发重渲染）。 */
export const EMPTY_SNAPSHOT_COMPARE: SnapshotCompare = { staged: [], unstaged: [] };

/** 变更的稳定标识：归一化后的相对路径（与产物归属共用同一套归一化）。 */
export function changeKey(change: FileChangeInfo): string {
  return normalizeArtifactKey(change.relative_path) ?? change.relative_path;
}

/**
 * 按相对路径排序，返回新数组。
 *
 * `compare` 的行序来自 `git status`，不保证稳定；界面按字母序（大小写不敏感，
 * 同键再按原字符串）排，避免每次刷新行序跳动。
 */
export function sortChanges(changes: FileChangeInfo[]): FileChangeInfo[] {
  return [...changes].sort((a, b) => {
    const left = changeKey(a).toLowerCase();
    const right = changeKey(b).toLowerCase();
    if (left !== right) return left < right ? -1 : 1;
    const rawLeft = changeKey(a);
    const rawRight = changeKey(b);
    if (rawLeft === rawRight) return 0;
    return rawLeft < rawRight ? -1 : 1;
  });
}

/** 变更总数（待处理 + 已接受）。 */
export function changeCount(compare: SnapshotCompare): number {
  return compare.unstaged.length + compare.staged.length;
}
