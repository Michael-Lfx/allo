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
  prompt?: string | null;
  trajectory_event_count?: number;
  artifact_count?: number;
  has_trace?: boolean;
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
};

export type StartEvalRunRequest = {
  suite: string;
  provider_id?: ProviderId;
  model?: string;
  limit?: number;
  task_profile?: string;
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
  getRun: (runId: string) =>
    httpRequest<EvalRunView>('GET', `${BASE}/runs/${encodeURIComponent(runId)}`, undefined, SILENT_403),
  cancelRun: (runId: string) =>
    httpRequest<EvalRunView>('POST', `${BASE}/runs/${encodeURIComponent(runId)}/cancel`),
  getCaseTrace: (runId: string, caseId: string) =>
    httpRequest<EvalCaseTraceView>(
      'GET',
      `${BASE}/runs/${encodeURIComponent(runId)}/cases/${encodeURIComponent(caseId)}/trace`,
      undefined,
      { silentStatuses: [403, 404] }
    ),
};
