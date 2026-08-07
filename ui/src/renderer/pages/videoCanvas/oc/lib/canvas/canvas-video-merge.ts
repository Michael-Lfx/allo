/**
 * Merge multiple canvas videos into one.
 *
 * Prefer allo's local ffmpeg concat (`POST /api/video-canvas/media/concat`) so
 * desktop / Electron builds never hang on unpkg `@ffmpeg/core` WASM downloads.
 * Fall back to in-browser ffmpeg.wasm only when media ids are unavailable.
 */

import { FFmpeg } from "@ffmpeg/ffmpeg";
import { fetchFile, toBlobURL } from "@ffmpeg/util";
import { getMediaBlob } from "@oc/services/file-storage";
import { resourceIdFromStorageKey, resourceFileUrl } from "@oc/services/api/resources";
import { concatCanvasMedia, resolveCanvasUrl } from "@renderer/pages/videoCanvas/api";
import { buildBackendAuthHeaders } from "@/common/adapter/httpBridge";

export type MergeVideoInput = { id: string; url?: string; storageKey?: string };
export type MergeVideoProgress = { phase: "loading" | "reading" | "encoding"; progress: number };

let ffmpegPromise: Promise<FFmpeg> | null = null;

function mediaIdFromInput(input: MergeVideoInput): string | null {
  const fromKey = resourceIdFromStorageKey(input.storageKey);
  if (fromKey) return fromKey;
  const url = input.url || "";
  const match = url.match(/\/api\/video-canvas\/media\/([^/?#]+)/);
  return match?.[1] ? decodeURIComponent(match[1]) : null;
}

async function mergeViaServerConcat(
  inputs: MergeVideoInput[],
  onProgress?: (progress: MergeVideoProgress) => void,
): Promise<Blob> {
  const mediaIds = inputs.map(mediaIdFromInput);
  if (mediaIds.some((id) => !id)) {
    throw new Error("missing media id");
  }
  onProgress?.({ phase: "loading", progress: 5 });
  onProgress?.({ phase: "reading", progress: 25 });
  const meta = await concatCanvasMedia(
    mediaIds as string[],
    `合并成片 · ${inputs.length} 段`,
  );
  onProgress?.({ phase: "encoding", progress: 70 });
  const absolute = resolveCanvasUrl(meta.url) || resourceFileUrl(meta.media_id);
  const headers = { ...buildBackendAuthHeaders("GET") };
  const response = await fetch(absolute, {
    method: "GET",
    headers,
    credentials: "omit",
  });
  if (!response.ok) {
    throw new Error(`合并结果下载失败（${response.status}）`);
  }
  const blob = await response.blob();
  onProgress?.({ phase: "encoding", progress: 100 });
  return blob;
}

async function loadFFmpeg(onProgress?: (progress: MergeVideoProgress) => void) {
  if (!ffmpegPromise) {
    ffmpegPromise = (async () => {
      const ffmpeg = new FFmpeg();
      const baseURL = "https://unpkg.com/@ffmpeg/core@0.12.10/dist/umd";
      onProgress?.({ phase: "loading", progress: 0 });
      await ffmpeg.load({
        coreURL: await toBlobURL(`${baseURL}/ffmpeg-core.js`, "text/javascript"),
        wasmURL: await toBlobURL(`${baseURL}/ffmpeg-core.wasm`, "application/wasm"),
      });
      return ffmpeg;
    })();
  }
  try {
    return await ffmpegPromise;
  } catch (error) {
    ffmpegPromise = null;
    throw error;
  }
}

async function mergeViaWasm(
  inputs: MergeVideoInput[],
  onProgress?: (progress: MergeVideoProgress) => void,
): Promise<Blob> {
  const ffmpeg = await loadFFmpeg(onProgress);
  const files: string[] = [];
  try {
    for (let index = 0; index < inputs.length; index += 1) {
      const input = inputs[index];
      const storedBlob = input.storageKey ? await getMediaBlob(input.storageKey).catch(() => null) : null;
      let remoteBlob: Blob | null = null;
      if (!storedBlob && input.url) {
        const absolute = resolveCanvasUrl(input.url) || input.url;
        const headers = absolute.includes("/api/video-canvas/")
          ? { ...buildBackendAuthHeaders("GET") }
          : undefined;
        const response = await fetch(absolute, {
          method: "GET",
          headers,
          credentials: "omit",
        });
        if (!response.ok) throw new Error(`视频资源请求失败（${response.status}）`);
        remoteBlob = await response.blob();
      }
      const blob = storedBlob || remoteBlob;
      if (!blob) throw new Error(`无法读取第 ${index + 1} 个视频`);
      const name = `input-${index}.mp4`;
      await ffmpeg.writeFile(name, await fetchFile(blob));
      files.push(name);
      onProgress?.({ phase: "reading", progress: Math.round(((index + 1) / inputs.length) * 45) });
    }
    const concatList = files.map((file) => `file '${file}'`).join("\n");
    await ffmpeg.writeFile("concat.txt", concatList);
    onProgress?.({ phase: "encoding", progress: 55 });
    let exitCode = await ffmpeg.exec([
      "-f",
      "concat",
      "-safe",
      "0",
      "-i",
      "concat.txt",
      "-c",
      "copy",
      "merged.mp4",
    ]);
    if (exitCode !== 0) {
      exitCode = await ffmpeg.exec([
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        "concat.txt",
        "-c:v",
        "libx264",
        "-c:a",
        "aac",
        "-movflags",
        "+faststart",
        "merged.mp4",
      ]);
    }
    if (exitCode !== 0) throw new Error("视频编码失败，请确认视频编码格式兼容");
    const output = await ffmpeg.readFile("merged.mp4");
    onProgress?.({ phase: "encoding", progress: 100 });
    return new Blob([output as BlobPart], { type: "video/mp4" });
  } finally {
    await Promise.all(
      [...files, "concat.txt", "merged.mp4"].map((file) =>
        ffmpeg.deleteFile(file).catch(() => undefined),
      ),
    );
  }
}

export async function mergeVideos(
  inputs: MergeVideoInput[],
  onProgress?: (progress: MergeVideoProgress) => void,
) {
  if (inputs.length < 2) throw new Error("至少选择 2 个视频才能合并");

  // Primary path: allo backend ffmpeg (same as Agent concat) — no CDN / WASM.
  try {
    return await mergeViaServerConcat(inputs, onProgress);
  } catch (serverError) {
    console.warn("[canvas-video-merge] server concat failed, trying wasm fallback", serverError);
  }

  try {
    return await mergeViaWasm(inputs, onProgress);
  } catch (wasmError) {
    const serverHint =
      "本机合并失败；浏览器 ffmpeg 回退也失败（常见于桌面端无法下载 unpkg WASM）。请确认后端 /api/video-canvas/media/concat 可用。";
    const detail = wasmError instanceof Error ? wasmError.message : String(wasmError);
    throw new Error(`${serverHint} ${detail}`);
  }
}
