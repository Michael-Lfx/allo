import { httpRequest, getBaseUrl } from '@/common/adapter/httpBridge';

const BASE = '/api/briefing';

export type BriefingStatus =
  | 'idle'
  | 'researching'
  | 'scripting'
  | 'aligning'
  | 'composing'
  | 'succeeded'
  | 'failed'
  | 'hold'
  | 'cancelled'
  | 'interrupted';

export interface BriefingSessionSummary {
  id: string;
  title: string;
  stage: string;
  status: BriefingStatus | string;
  final_video?: string | null;
  created_at: string;
  updated_at: string;
}

export interface BriefingSession {
  id: string;
  working_dir: string;
  title: string;
  intent: string;
  format_secs: number;
  research_depth: 'fast' | 'deep' | string;
  time_window_hours: number;
  source_urls: string[];
  plan_confirmed: boolean;
  stage: string;
  summary: string;
  status: BriefingStatus | string;
  final_video?: string | null;
  created_at: string;
  updated_at: string;
  tts_provider_id?: string | null;
  tts_model?: string | null;
  tts_voice?: string | null;
  image_provider_id?: string | null;
  image_model?: string | null;
}

export interface BriefingCitation {
  url: string;
  domain: string;
  excerpt?: string;
}

export interface BriefingBeat {
  id: string;
  spoken_text: string;
  on_screen: string;
  card: string;
  citations: BriefingCitation[];
}

export interface BriefingScript {
  format_secs: number;
  beats: BriefingBeat[];
  unknowns: string[];
}

export interface BriefingPlan {
  intent: string;
  questions: string[];
  time_window_hours: number;
  depth: string;
  confirmed: boolean;
}

export interface BriefingRunSnapshot {
  status: BriefingStatus | string;
  stage: string;
  message: string;
  final_video?: string | null;
}

export interface CreateBriefingBody {
  intent: string;
  title?: string;
  format_secs: number;
  research_depth: 'fast' | 'deep';
  time_window_hours: number;
  source_urls: string[];
  tts_provider_id?: string;
  tts_model?: string;
  tts_voice?: string | null;
  image_provider_id?: string;
  image_model?: string;
}

export interface BriefingModelsBody {
  tts_provider_id?: string | null;
  tts_model?: string | null;
  tts_voice?: string | null;
  image_provider_id?: string | null;
  image_model?: string | null;
}

export function briefingArtifactUrl(id: string, rel = 'briefing.mp4'): string {
  const base = getBaseUrl();
  return `${base}/api/briefing/sessions/${encodeURIComponent(id)}/artifacts/${rel}`;
}

export function resolveBriefingUrl(
  sessionId: string,
  path: string | null | undefined
): string | null {
  if (!path) return null;
  if (/^(https?:|blob:|data:)/i.test(path)) return path;
  return briefingArtifactUrl(sessionId);
}

export async function listBriefingSessions(): Promise<BriefingSessionSummary[]> {
  const data = await httpRequest<BriefingSessionSummary[] | { sessions: BriefingSessionSummary[] }>(
    'GET',
    `${BASE}/sessions`
  );
  return Array.isArray(data) ? data : data.sessions ?? [];
}

export async function createBriefing(body: CreateBriefingBody): Promise<BriefingSession> {
  return httpRequest<BriefingSession>('POST', `${BASE}/sessions`, body);
}

export async function updateBriefingModels(
  id: string,
  body: BriefingModelsBody
): Promise<BriefingSession> {
  return httpRequest<BriefingSession>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(id)}/models`,
    body
  );
}

export async function getBriefing(id: string): Promise<BriefingSession> {
  return httpRequest<BriefingSession>('GET', `${BASE}/sessions/${encodeURIComponent(id)}`);
}

export async function getBriefingStatus(id: string): Promise<BriefingRunSnapshot> {
  return httpRequest<BriefingRunSnapshot>(
    'GET',
    `${BASE}/sessions/${encodeURIComponent(id)}/status`
  );
}

export async function getBriefingPlan(id: string): Promise<BriefingPlan> {
  return httpRequest<BriefingPlan>('GET', `${BASE}/sessions/${encodeURIComponent(id)}/plan`);
}

export async function confirmBriefingPlan(id: string, plan?: BriefingPlan): Promise<BriefingPlan> {
  return httpRequest<BriefingPlan>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(id)}/plan`,
    plan ?? null
  );
}

export async function getBriefingScript(id: string): Promise<BriefingScript> {
  return httpRequest<BriefingScript>('GET', `${BASE}/sessions/${encodeURIComponent(id)}/script`);
}

export async function saveBriefingScript(id: string, script: BriefingScript): Promise<BriefingScript> {
  return httpRequest<BriefingScript>(
    'POST',
    `${BASE}/sessions/${encodeURIComponent(id)}/script`,
    script
  );
}

export async function runBriefing(id: string): Promise<void> {
  await httpRequest('POST', `${BASE}/sessions/${encodeURIComponent(id)}/run`, {
    confirm_plan: true,
  });
}

export async function cancelBriefing(id: string): Promise<void> {
  await httpRequest('POST', `${BASE}/sessions/${encodeURIComponent(id)}/cancel`);
}

export function briefingWorkspacePath(id: string): string {
  return `/video-generation/briefing/${encodeURIComponent(id)}`;
}
