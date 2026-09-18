import type { WorkspaceFlatFile } from "./protocol";

/**
 * R15（W10）附件的**客户端前置校验**。
 *
 * 口径刻意与运行时（`nomifun-ai-agent` 的 `manager/nomi/image_attachments.rs`）
 * 逐条对齐，这样界面能提前说清「为什么这个文件不能附」，而不是发出去之后再收到一个
 * 看不懂的 400：
 *  - 只有 **PNG / JPEG / WebP** 会被真正送进模型（`.png` `.jpg` `.jpeg` `.webp`）；
 *  - 运行时**显式拒绝**的图片格式（gif/bmp/tif/tiff/ico/avif/heic/heif/svg）在这里
 *    就给出原因，不让用户以为附上了；
 *  - **非图片**文件运行时直接忽略（不报错、也不进模型上下文），所以界面不提供它们——
 *    「附了但模型没看见」比「不让附」更糟。
 *
 * 这些只是**前置**校验，不是唯一防线：服务端仍做路径准入（必须落在会话工作区内），
 * 运行时仍做字节 / 像素 / 数量校验。
 */

/** 运行时支持的图片扩展名（`classify_extension` 的 `Ok(Some(_))` 分支）。 */
export const SUPPORTED_ATTACHMENT_EXTENSIONS = ["png", "jpg", "jpeg", "webp"] as const;

/** 看起来是图片、但运行时明确拒绝的扩展名（`classify_extension` 的 `Err` 分支）。 */
export const REJECTED_IMAGE_EXTENSIONS = [
  "gif",
  "bmp",
  "tif",
  "tiff",
  "ico",
  "avif",
  "heic",
  "heif",
  "svg",
] as const;

/** 与运行时 `MAX_IMAGE_ATTACHMENTS`、服务端上限同值。 */
export const MAX_ATTACHMENTS = 10;

export type AttachmentEligibility =
  | { kind: "supported"; path: string; name: string }
  | { kind: "rejected-image"; path: string; name: string; extension: string }
  | { kind: "not-an-image"; path: string; name: string };

/** 扩展名（小写、不含点）；没有扩展名返回空串。 */
export function attachmentExtension(pathOrName: string): string {
  const base = pathOrName.replace(/\\/g, "/").split("/").pop() ?? "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

/** 展示用文件名（去掉目录部分）。 */
export function attachmentName(path: string): string {
  return path.replace(/\\/g, "/").split("/").pop() || path;
}

/** 单个路径的可附性判定。 */
export function classifyAttachment(path: string): AttachmentEligibility {
  const extension = attachmentExtension(path);
  const name = attachmentName(path);
  if ((SUPPORTED_ATTACHMENT_EXTENSIONS as readonly string[]).includes(extension)) {
    return { kind: "supported", path, name };
  }
  if ((REJECTED_IMAGE_EXTENSIONS as readonly string[]).includes(extension)) {
    return { kind: "rejected-image", path, name, extension };
  }
  return { kind: "not-an-image", path, name };
}

/**
 * 从工作区文件列表里挑出**可以附**的文件，并按剩余名额截断；不可附的连同原因一起
 * 返回，界面据此说明原因（而不是把行藏起来让人猜）。
 *
 * 已选中的路径从候选里排除（避免重复），且**不占用** `remaining` 之外的名额。
 */
export function attachmentCandidates(
  files: WorkspaceFlatFile[],
  alreadyPicked: string[],
  limit = MAX_ATTACHMENTS,
): { pickable: AttachmentEligibility[]; skipped: AttachmentEligibility[]; remaining: number } {
  const picked = new Set(alreadyPicked);
  const candidates = files
    .map((file) => classifyAttachment(file.full_path))
    .filter((entry) => !picked.has(entry.path));
  const remaining = Math.max(0, limit - alreadyPicked.length);
  return {
    pickable: candidates.filter((entry) => entry.kind === "supported").slice(0, remaining),
    skipped: candidates.filter((entry) => entry.kind !== "supported"),
    remaining,
  };
}
