import { httpRequest } from '@/common/adapter/httpBridge';
import type { ProviderId } from '@/common/types/ids';

const BASE = '/api/debug/agent-evals';
const SILENT_403 = { silentStatuses: [403] };

export type EvalSuiteDescriptor = {
  id: string;
  title: string;
  kind: string;
  default_task_profile: string;
  source_url?: string | null;
  default_limit: number;
  max_limit: number;
  notes: string;
  requires_download: boolean;
  cached: boolean;
  tier?: string;
  default_trials?: number;
  requires_sandbox?: boolean;
};

export type EvalScorerView = {
  scorer_type: string;
  passed: boolean;
  detail?: string | null;
};

export type EvalTrajectoryEventView = {
  kind: string;
  ts_ms: number;
  tool_use_id?: string | null;
  name?: string | null;
  input?: string | null;
  content?: string | null;
  is_error?: boolean | null;
};

export type EvalArtifactView = {
  path: string;
  size_bytes: number;
  kind: string;
  preview?: string | null;
};

export type EvalCaseTraceView = {
  case_id: string;
  live: boolean;
  assistant_text: string;
  events: EvalTrajectoryEventView[];
  artifacts: EvalArtifactView[];
  conversation_id?: string | null;
};

export type EvalCaseView = {
  case_id: string;
  category: string;
  success: boolean;
  elapsed_ms: number;
  turns: number;
  tool_call_count: number;
  input_tokens: number;
  output_tokens: number;
  tool_error_count: number;
  stop_reason?: string | null;
  error?: string | null;
  scorer_results: EvalScorerView[];
  advisory_results?: EvalScorerView[];
  trial?: number;
  prompt?: string | null;
  trajectory_event_count?: number;
  artifact_count?: number;
  has_trace?: boolean;
  conversation_id?: string | null;
};

export type EvalCategoryView = {
  category: string;
  total: number;
  passed: number;
  success_rate: number;
};

export type EvalSummaryView = {
  total_cases: number;
  passed: number;
  failed: number;
  success_rate: number;
  avg_turns: number;
  avg_elapsed_ms: number;
  avg_input_tokens: number;
  avg_output_tokens: number;
  by_category: EvalCategoryView[];
  unique_cases?: number;
  n_trials?: number;
  pass_at_1?: number;
  pass_hat_k?: number;
};

export type EvalRunView = {
  run_id: string;
  status: string;
  suite: string;
  model?: string | null;
  provider_id?: string | null;
  planned: number;
  completed: number;
  passed: number;
  failed: number;
  current_case_id?: string | null;
  error?: string | null;
  summary?: EvalSummaryView | null;
  cases: EvalCaseView[];
  current_trace?: EvalCaseTraceView | null;
  current_conversation_id?: string | null;
  workspace_label?: string | null;
  workspace_path?: string | null;
};

export type EvalRunListItem = {
  run_id: string;
  suite: string;
  status: string;
  passed: number;
  failed: number;
  pass_at_1: number;
};

export type EvalRunDiffView = {
  a: string;
  b: string;
  flipped: Array<{ case_id: string; a_success: boolean; b_success: boolean }>;
  pass_at_1_delta: number;
};

export type StartEvalRunRequest = {
  suite: string;
  provider_id?: ProviderId;
  model?: string;
  limit?: number;
  task_profile?: string;
  n_trials?: number;
};

export const evalApi = {
  listSuites: () => httpRequest<EvalSuiteDescriptor[]>('GET', `${BASE}/suites`, undefined, SILENT_403),
  pullDataset: (suite: string, limit?: number) => {
    const query = limit != null ? `?limit=${limit}` : '';
    return httpRequest<{ suite: string; corpus_version: string; cases: number }>(
      'POST',
      `${BASE}/datasets/${encodeURIComponent(suite)}/pull${query}`
    );
  },
  startRun: (request: StartEvalRunRequest) =>
    httpRequest<EvalRunView>('POST', `${BASE}/runs`, request),
  latestRun: () =>
    httpRequest<EvalRunView | null>('GET', `${BASE}/runs`, undefined, SILENT_403),
  history: () =>
    httpRequest<EvalRunListItem[]>('GET', `${BASE}/history`, undefined, SILENT_403),
  diffRuns: (a: string, b: string) =>
    httpRequest<EvalRunDiffView>(
      'GET',
      `${BASE}/runs/${encodeURIComponent(a)}/diff/${encodeURIComponent(b)}`,
      undefined,
      SILENT_403
    ),
  getRun: (runId: string) =>
    httpRequest<EvalRunView>('GET', `${BASE}/runs/${encodeURIComponent(runId)}`, undefined, SILENT_403),
  cancelRun: (runId: string) =>
    httpRequest<EvalRunView>('POST', `${BASE}/runs/${encodeURIComponent(runId)}/cancel`),
  getCaseTrace: (runId: string, caseId: string, trial?: number) => {
    const query = trial != null ? `?trial=${trial}` : '';
    return httpRequest<EvalCaseTraceView>(
      'GET',
      `${BASE}/runs/${encodeURIComponent(runId)}/cases/${encodeURIComponent(caseId)}/trace${query}`,
      undefined,
      { silentStatuses: [403, 404] }
    );
  },
  getCaseObservation: (runId: string, caseId: string, limit?: number) => {
    const query = limit != null ? `?limit=${limit}` : '';
    return httpRequest<{
      recorder_health: { status: string; last_error?: string | null };
      summary: {
        turn_count: number;
        model_call_count: number;
        tool_count: number;
        active_duration_ms: number;
        integrity: string;
        coverage: string;
        max_event_seq: number;
      };
      turns: Array<{
        root_turn_id: string;
        session_kind?: string | null;
        status?: string | null;
        integrity?: string | null;
        model_calls?: unknown[];
      }>;
    }>(
      'GET',
      `${BASE}/runs/${encodeURIComponent(runId)}/cases/${encodeURIComponent(caseId)}/observation${query}`,
      undefined,
      { silentStatuses: [403, 404] }
    );
  },
  reportCase: (body: {
    case_id: string;
    suite: string;
    category: string;
    error?: string | null;
    prompt: string;
    scorer_json: string;
  }) => httpRequest<unknown>('POST', `${BASE}/report-case`, body),
  syncPrivate: () => httpRequest<number>('POST', `${BASE}/private/sync`),
};
