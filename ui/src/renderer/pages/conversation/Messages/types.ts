

/**
 * Type definitions for message tool results
 * 消息工具结果类型定义
 */

export interface ImageGenerationResult {
  img_url?: string;
  relative_path?: string;
  error?: string;
}

export interface VideoGenerationResult {
  success?: boolean;
  video?: string;
  local_path?: string;
  error?: string;
  assets?: Array<{
    url?: string;
    local_path?: string;
    kind?: string;
  }>;
}

export interface WriteFileResult {
  file_diff: string;
  file_name: string;
  [key: string]: unknown;
}
