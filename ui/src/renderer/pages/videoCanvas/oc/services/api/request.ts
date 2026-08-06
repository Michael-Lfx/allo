/**
 * Compatibility shim for open-ai-canvas API helpers.
 * Allo canvas uses `/api/video-canvas/*` via httpBridge — stub modules should not
 * call the original Go backend through this client.
 */

import axios from 'axios';
import { getBaseUrl } from '@/common/adapter/httpBridge';

export type ApiParams = Record<string, string | string[] | number | number[] | undefined>;

export type BackendEnvelope<T> = {
  code: number;
  data: T;
  msg: string;
};

export const apiBaseURL = `${getBaseUrl().replace(/\/+$/, '')}/api/video-canvas`;

export const apiClient = axios.create({
  baseURL: apiBaseURL,
  withCredentials: true,
});

export async function request<T>(promise: Promise<{ data: BackendEnvelope<T> }>): Promise<T> {
  try {
    const response = await promise;
    if (response.data.code !== 0) throw new Error(response.data.msg || '请求失败');
    return response.data.data;
  } catch (error) {
    if (axios.isAxiosError<BackendEnvelope<unknown>>(error)) {
      throw new Error(error.response?.data?.msg || error.message || '请求失败');
    }
    throw error;
  }
}

export function compactApiParams(params: ApiParams) {
  return Object.fromEntries(
    Object.entries(params).filter(
      ([, value]) =>
        value !== '' && value !== undefined && (!Array.isArray(value) || value.length > 0)
    )
  ) as ApiParams;
}

export function serializeApiParams(params?: ApiParams) {
  const queryParams = new URLSearchParams();
  for (const [key, value] of Object.entries(params || {})) {
    if (value === undefined) continue;
    if (Array.isArray(value)) value.forEach((item) => queryParams.append(key, String(item)));
    else queryParams.set(key, String(value));
  }
  return queryParams;
}
