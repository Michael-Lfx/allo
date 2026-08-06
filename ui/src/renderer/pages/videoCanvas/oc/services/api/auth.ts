/**
 * Auth / admin API stub for allo canvas.
 * Enables canvas task-center features locally; admin APIs reject.
 */

import type { ModelChannel } from '@oc/stores/use-config-store';
import type { CreditLedgerEntry } from '@oc/services/api/wallet';
import type { GenerationTask, TaskStatus } from '@oc/services/api/task-center';
import type { CanvasDrawingEngineSetting } from '@oc/lib/canvas/canvas-drawing-engine';
import type { FeatureAvailability } from '@oc/stores/use-user-store';

export type LocalUser = {
  id: string;
  username: string;
  email?: string;
  displayName: string;
  avatarUrl?: string;
  identityProvider?: string;
  identityId?: string;
  identityUsername?: string;
  role: 'admin' | 'user';
  status: 'active' | 'disabled';
  lastLoginAt?: string;
  createdAt: string;
  updatedAt: string;
};

export type AdminUser = LocalUser & {
  availableMicrocredits: number;
  reservedMicrocredits: number;
};

export type AuthSessionPayload = {
  user: LocalUser | null;
  systemChannels?: ModelChannel[];
  runtimeLimits?: RuntimeLimits;
  drawingEngine?: CanvasDrawingEngineSetting;
  features?: FeatureAvailability;
};

export type RuntimeLimits = {
  activeTaskLimit: number;
  resourceUploadMB: number;
  sessionUploadMB: number;
};

export type ApiCallLog = {
  id: string;
  userId: string;
  userDisplayName?: string;
  userAccount?: string;
  channelId: string;
  channelName: string;
  taskId?: string;
  taskStatus?: TaskStatus;
  source: string;
  capability: 'text' | 'image' | 'video' | 'audio' | '';
  operation?: string;
  requestKind: 'create' | 'poll' | 'download' | 'repair' | '';
  billable: boolean;
  apiFormat: string;
  method: string;
  path: string;
  model: string;
  status: 'succeeded' | 'failed';
  statusCode: number;
  durationMs: number;
  pollCount: number;
  providerStatus?: string;
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  usageAvailable: boolean;
  mediaCount: number;
  mediaPreviewUrl?: string;
  mediaPreviewKind?: 'image' | 'video';
  videoSeconds: number;
  providerRequestId?: string;
  estimatedCostMicros: number;
  costAvailable: boolean;
  currency?: string;
  errorCode?: string;
  error?: string;
  concurrencyLimit: number;
  upstreamUrl: string;
  requestContentType?: string;
  requestBody?: string;
  responseBody?: string;
  startedAt?: string;
  createdAt: string;
};

export type AdminProviderTaskQueryResult = {
  task: GenerationTask;
  providerStatus: string;
  recovered: boolean;
  billingSettled: boolean;
};

export type AdminAuditEvent = {
  id: string;
  actorUserId: string;
  action: string;
  targetType: string;
  targetId: string;
  summary: string;
  metadataJson?: string;
  createdAt: string;
};

export type AdminUserDetail = {
  user: LocalUser;
  account: { userId: string; availableMicrocredits: number; reservedMicrocredits: number; version: number };
  counts: { ledgerEntries: number; tasks: number; apiCalls: number; auditEvents: number };
  storageUsage: {
    assetCount: number;
    assetBytes: number;
    canvasCount: number;
    canvasBytes: number;
    sessionCount: number;
    sessionBytes: number;
    taskCount: number;
    taskBytes: number;
    apiCallCount: number;
  };
  storedFileBytes: number;
  dailyUploadBytes: number;
  quota: RuntimeResourcePolicy;
};

export type AdminUserTask = {
  id: string;
  type: string;
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
  stage: string;
  progress: number;
  model?: string;
  providerRequestId?: string;
  createdAt: string;
};

export type AnalyticsFilters = {
  from?: string;
  to?: string;
  userId?: string;
  model?: string;
  channelId?: string;
  capability?: string;
};

export type AdminReferenceData = {
  users: Array<{ id: string; username: string; displayName: string }>;
  channels: Array<{ id: string; name: string; models: string[] }>;
};

export type AdminAnalytics = {
  from: string;
  to: string;
  kpi: {
    activeUsers: number;
    dau: number;
    wau: number;
    mau: number;
    generationTasks: number;
    upstreamRequests: number;
    successRate: number;
    p95DurationMs: number;
    currentQueuedTasks: number;
    estimatedCostMicros: number;
    costAvailable: boolean;
    currency?: string;
  };
  trend: Array<{
    day: string;
    tasks: number;
    requests: number;
    activeUsers: number;
    requestSuccessRate: number;
  }>;
  models: Array<{
    model: string;
    capability: string;
    tasks: number;
    requests: number;
    uniqueUsers: number;
    taskSuccessRate: number;
    requestSuccessRate: number;
    p50DurationMs: number;
    p95DurationMs: number;
    inputTokens: number;
    outputTokens: number;
    cachedTokens: number;
    usageAvailable: boolean;
    mediaCount: number;
    videoSeconds: number;
    estimatedCostMicros: number;
    costAvailable: boolean;
    currency?: string;
  }>;
  users: Array<{
    userId: string;
    name: string;
    activeDays: number;
    tasks: number;
    agentMessages: number;
    canvasDays: number;
    assets: number;
    resources: number;
    commonModel?: string;
  }>;
  failures: Array<{ type: string; model: string; count: number; lastError?: string; lastSeenAt: string }>;
};

export type ModelPricing = {
  id: string;
  channelId?: string;
  model: string;
  capability: 'text' | 'image' | 'video' | 'audio';
  currency: string;
  inputPerMillionMicros: number;
  outputPerMillionMicros: number;
  cachedPerMillionMicros: number;
  perRequestMicros: number;
  perMediaMicros: number;
  perVideoSecondMicros: number;
  createdAt: string;
  updatedAt: string;
};

export type PromptTemplate = {
  id: string;
  operation: string;
  name: string;
  version: number;
  content: string;
  outputType: 'json' | 'text';
  enabled: boolean;
  createdBy?: string;
  createdAt: string;
  updatedAt: string;
};

export type PromptTemplateVariable = {
  label: string;
  placeholder: string;
};

export type PromptOperationDefinition = {
  operation: string;
  label: string;
  category: string;
  description: string;
  outputType: 'json' | 'text';
  schemaKey?: string;
  variables: PromptTemplateVariable[];
  outputContract: string;
};

export type UserPromptCustomization = {
  id: string;
  operation: string;
  mode: 'inherit' | 'append' | 'rewrite';
  content: string;
  baseTemplateId: string;
  updatedAt: string;
};

export type UserPromptPreference = {
  definition: PromptOperationDefinition;
  template: PromptTemplate | null;
  customization?: UserPromptCustomization;
  outdated: boolean;
};

export type AdminOSSSetting = {
  enabled: boolean;
  provider: 'aliyun';
  region: string;
  endpoint: string;
  bucket: string;
  accessKeyId: string;
  accessKeySecret?: string;
  hasAccessKeySecret: boolean;
  publicBaseUrl: string;
  pathPrefix: string;
  updatedBy?: string;
  createdAt?: string;
  updatedAt?: string;
};

export type RuntimeResourcePolicy = {
  resourceUploadMB: number;
  sessionUploadMB: number;
  generatedFileMB: number;
  dailyUploadMB: number;
  storedFileGB: number;
  structuredDataMB: number;
  taskDataGB: number;
  assetCount: number;
  canvasCount: number;
  sessionCount: number;
  taskCount: number;
  apiCallLogCount: number;
};

export type RuntimeTaskPolicy = {
  workerConcurrency: number;
  channelConcurrency: number;
  activeTaskLimit: number;
  imageTimeoutMinutes: number;
  textTimeoutMinutes: number;
  audioTimeoutMinutes: number;
  videoTimeoutMinutes: number;
  storyboardTimeoutMinutes: number;
  defaultTimeoutMinutes: number;
};

export type RuntimeRequestPolicy = {
  taskCreatePerMinute: number;
  sessionCreatePerMinute: number;
  resourceUploadPerMinute: number;
  resourceImportPerMinute: number;
  sessionFilePerMinute: number;
  assetWritePerMinute: number;
  canvasWritePerMinute: number;
  registerPerHour: number;
  emailCodePerHour: number;
  loginIPPerTenMinutes: number;
  loginAccountPerTenMinutes: number;
  systemRelayPerMinute: number;
  customRelayPerMinute: number;
  customRelayConcurrency: number;
  customRelayRequestMB: number;
  customRelayResponseMB: number;
  customRelayTimeoutMinutes: number;
  systemRelayRequestMB: number;
  systemRelayResponseMB: number;
  channelCircuitFailureCount: number;
  channelCircuitOpenSeconds: number;
};

export type RuntimePolicySetting = {
  resource: RuntimeResourcePolicy;
  task: RuntimeTaskPolicy;
  request: RuntimeRequestPolicy;
  configured?: boolean;
  updatedBy?: string;
  createdAt?: string;
  updatedAt?: string;
};

export type AdminListParams = {
  keyword?: string;
  status?: string;
  role?: string;
  page?: number;
  limit?: number;
};

const ENABLED_FEATURES: FeatureAvailability = {
  shortDramaEnabled: false,
  taskCenterEnabled: true,
  creditsEnabled: false,
};

const adminUnavailable = () => Promise.reject(new Error('Admin APIs are not available in allo canvas'));

export function getAuthSettings() {
  return Promise.resolve({
    firstUser: false,
    registrationEnabled: false,
    linuxdoEnabled: false,
    emailEnabled: false,
    emailCodeRequired: false,
  });
}

export function linuxDOLoginURL(next: string) {
  return `/auth/linuxdo/start?next=${encodeURIComponent(next)}`;
}

export function getAuthSession() {
  return Promise.resolve({
    user: null,
    systemChannels: [],
    runtimeLimits: { activeTaskLimit: 5, resourceUploadMB: 50, sessionUploadMB: 32 },
    features: ENABLED_FEATURES,
  } as AuthSessionPayload);
}

export function getSystemChannels() {
  return Promise.resolve({ channels: [] as ModelChannel[] });
}

export function getFeatureAvailability() {
  return Promise.resolve({ features: ENABLED_FEATURES });
}

export function getAdminFeatureAvailability() {
  return Promise.resolve({ features: ENABLED_FEATURES });
}

export function updateAdminFeatureAvailability(
  _features: Pick<FeatureAvailability, 'shortDramaEnabled' | 'taskCenterEnabled' | 'creditsEnabled'>
) {
  return adminUnavailable() as Promise<{ features: FeatureAvailability }>;
}

export function login(_input: { username: string; password: string }) {
  return Promise.reject(new Error('Auth login is not available in allo canvas'));
}

export function sendRegistrationEmailCode(_email: string) {
  return Promise.reject(new Error('Registration is not available in allo canvas'));
}

export function register(_input: {
  username: string;
  email?: string;
  emailCode?: string;
  displayName?: string;
  password: string;
}) {
  return Promise.reject(new Error('Registration is not available in allo canvas'));
}

export function logout() {
  return Promise.resolve({ ok: true });
}

export function listAdminUsers(_params: AdminListParams = {}) {
  return Promise.resolve({ users: [] as AdminUser[], total: 0, page: 1, limit: 30 });
}

export function createAdminUser(_input: {
  username: string;
  displayName: string;
  email?: string;
  password: string;
  role: LocalUser['role'];
  status: LocalUser['status'];
}) {
  return adminUnavailable() as Promise<{ user: AdminUser }>;
}

export function getAdminReferences() {
  return Promise.resolve({ users: [], channels: [] } as AdminReferenceData);
}

export function getAdminUserDetail(_id: string) {
  return adminUnavailable() as Promise<AdminUserDetail>;
}

export function listAdminUserLedger(
  _id: string,
  _params: { page?: number; limit?: number; type?: string } = {}
) {
  return Promise.resolve({ entries: [] as CreditLedgerEntry[], total: 0, page: 1, limit: 30 });
}

export function listAdminUserTasks(_id: string, _params: { page?: number; limit?: number } = {}) {
  return Promise.resolve({ tasks: [] as AdminUserTask[], total: 0, page: 1, limit: 30 });
}

export function listAdminUserAuditEvents(_id: string, _params: { page?: number; limit?: number } = {}) {
  return Promise.resolve({ events: [] as AdminAuditEvent[], total: 0, page: 1, limit: 30 });
}

export function updateAdminUser(
  _id: string,
  _input: Partial<Pick<LocalUser, 'displayName' | 'email' | 'role' | 'status'>> & { password?: string }
) {
  return adminUnavailable() as Promise<{ user: LocalUser }>;
}

export function deleteAdminUser(_id: string) {
  return Promise.resolve({ ok: true });
}

export function bulkDisableAdminUsers(_userIds: string[]) {
  return Promise.resolve({ users: [] as LocalUser[], disabledCount: 0 });
}

export function listAdminChannels(_params: AdminListParams = {}) {
  return Promise.resolve({ channels: [] as ModelChannel[], total: 0, page: 1, limit: 30 });
}

export function createAdminChannel(_input: Partial<ModelChannel> & { useGlobalConcurrency?: boolean }) {
  return adminUnavailable() as Promise<{ channel: ModelChannel }>;
}

export function updateAdminChannel(
  _id: string,
  _input: Partial<ModelChannel> & { useGlobalConcurrency?: boolean }
) {
  return adminUnavailable() as Promise<{ channel: ModelChannel }>;
}

export function deleteAdminChannel(_id: string) {
  return Promise.resolve({ ok: true });
}

export function listAdminPromptTemplates() {
  return Promise.resolve({
    templates: [] as PromptTemplate[],
    definitions: [] as PromptOperationDefinition[],
  });
}

export function createAdminPromptTemplate(
  _input: Pick<PromptTemplate, 'operation' | 'name' | 'content'> & { enabled?: boolean }
) {
  return adminUnavailable() as Promise<{ template: PromptTemplate }>;
}

export function updateAdminPromptTemplate(
  _id: string,
  _input: Pick<PromptTemplate, 'operation' | 'name' | 'content'> & { enabled?: boolean }
) {
  return adminUnavailable() as Promise<{ template: PromptTemplate }>;
}

export function deleteAdminPromptTemplate(_id: string) {
  return Promise.resolve({ ok: true });
}

export function listUserPromptPreferences() {
  return Promise.resolve({ preferences: [] as UserPromptPreference[] });
}

export function updateUserPromptCustomization(
  _operation: string,
  _input: Pick<UserPromptCustomization, 'mode' | 'content'>
) {
  return adminUnavailable() as Promise<{ customization: UserPromptCustomization }>;
}

export function resetUserPromptCustomization(_operation: string) {
  return Promise.resolve({ ok: true });
}

export function getAdminOSSSetting() {
  return adminUnavailable() as Promise<{ setting: AdminOSSSetting }>;
}

export function updateAdminOSSSetting(_input: Partial<AdminOSSSetting>) {
  return adminUnavailable() as Promise<{ setting: AdminOSSSetting }>;
}

export function getAdminRuntimePolicySetting() {
  return adminUnavailable() as Promise<{ setting: RuntimePolicySetting }>;
}

export function getAdminSelfUseRuntimePolicy() {
  return adminUnavailable() as Promise<{ setting: RuntimePolicySetting }>;
}

export function updateAdminRuntimePolicySetting(
  _input: Pick<RuntimePolicySetting, 'resource' | 'task' | 'request'>
) {
  return adminUnavailable() as Promise<{ setting: RuntimePolicySetting }>;
}

export function resetAdminRuntimePolicySetting() {
  return adminUnavailable() as Promise<{ setting: RuntimePolicySetting }>;
}

export function getAdminDrawingEngineSetting() {
  return Promise.resolve({
    setting: { defaultEngine: 'excalidraw' } as CanvasDrawingEngineSetting,
  });
}

export function updateAdminDrawingEngineSetting(
  _input: Pick<CanvasDrawingEngineSetting, 'defaultEngine' | 'tldrawLicenseKey'>
) {
  return adminUnavailable() as Promise<{ setting: CanvasDrawingEngineSetting }>;
}

export function listAdminApiLogs(_params: AdminListParams = {}) {
  return Promise.resolve({ logs: [] as ApiCallLog[], total: 0, page: 1, limit: 30 });
}

export function getAdminApiLog(_id: string) {
  return adminUnavailable() as Promise<{ log: ApiCallLog }>;
}

export function queryAdminApiLogTask(_id: string) {
  return adminUnavailable() as Promise<AdminProviderTaskQueryResult>;
}

export async function exportAdminApiLogs(_params: AdminListParams & { ids?: string[] } = {}) {
  return new Blob([]);
}

export function getAdminAnalytics(_params: AnalyticsFilters) {
  return adminUnavailable() as Promise<AdminAnalytics>;
}

export async function exportAdminAnalytics(_params: AnalyticsFilters) {
  return new Blob([]);
}

export function listAdminModelPricings() {
  return Promise.resolve({ pricings: [] as ModelPricing[] });
}

export function createAdminModelPricing(_input: Omit<ModelPricing, 'id' | 'createdAt' | 'updatedAt'>) {
  return adminUnavailable() as Promise<{ pricing: ModelPricing }>;
}

export function updateAdminModelPricing(
  _id: string,
  _input: Omit<ModelPricing, 'id' | 'createdAt' | 'updatedAt'>
) {
  return adminUnavailable() as Promise<{ pricing: ModelPricing }>;
}

export function deleteAdminModelPricing(_id: string) {
  return Promise.resolve({ ok: true });
}
