/**
 * Allo stub: logical-model routing is Yingce admin/catalog infrastructure.
 * Keep CapabilitySpec types for model-selection compatibility; API calls no-op.
 */

export type InputConstraint = { min: number; max: number };
export type OptionConstraint = { values?: unknown[]; min?: number; max?: number; step?: number };
export type CapabilitySpec = {
  version: 1;
  capability: 'text' | 'image' | 'video' | 'audio';
  operations?: string[];
  inputs?: Record<string, InputConstraint>;
  options?: Record<string, OptionConstraint>;
};

export type ModelRequestIntent = {
  capability: CapabilitySpec['capability'];
  operation?: string;
  inputs?: Record<string, number>;
  options?: Record<string, unknown>;
};

export type PublicLogicalModel = {
  id: string;
  code: string;
  name: string;
  icon?: string;
  description: string;
  capability: CapabilitySpec['capability'];
  sortOrder: number;
  pricePolicy: 'channel' | 'unified';
  billingMode: 'fixed_request' | 'per_second' | 'token';
  unitPriceMicrocredits: number;
  inputPriceMicrocredits: number;
  outputPriceMicrocredits: number;
  cachedPriceMicrocredits: number;
  capabilitySpec: CapabilitySpec;
  capabilityProfiles: CapabilitySpec[];
  defaultOptions: Record<string, unknown>;
  available: boolean;
};

export type AdminLogicalRoute = {
  id: string;
  channelModelId: string;
  channelId: string;
  channelModelKey: string;
  channelModelName: string;
  enabled: boolean;
  priority: number;
  weight: number;
  available: boolean;
  capabilitySpec: CapabilitySpec;
};

export type AdminLogicalModel = PublicLogicalModel & {
  enabled: boolean;
  activeRevisionId: string;
  revisionVersion: number;
  configurationError?: string;
  availabilityError?: string;
  routes: AdminLogicalRoute[];
};

export type LogicalModelMutation = {
  code: string;
  name: string;
  icon: string;
  description: string;
  capability: CapabilitySpec['capability'];
  enabled: boolean;
  sortOrder: number;
  pricePolicy: PublicLogicalModel['pricePolicy'];
  billingMode: PublicLogicalModel['billingMode'];
  unitPriceMicrocredits: number;
  inputPriceMicrocredits: number;
  outputPriceMicrocredits: number;
  cachedPriceMicrocredits: number;
  capabilitySpec: CapabilitySpec;
  defaultOptions: Record<string, unknown>;
  routes: Array<{ channelModelId: string; enabled: boolean; priority: number; weight: number }>;
};

export type RouteSimulationResult = {
  productMatch: { matched: boolean; reasons?: string[] };
  candidates: Array<{
    routeId: string;
    channelModelId: string;
    channelModelKey: string;
    channelModelName: string;
    priority: number;
    weight: number;
    enabled: boolean;
    matched: boolean;
    blocked: boolean;
    inPool: boolean;
    reasons?: string[];
  }>;
};

export async function listLogicalModels() {
  return { models: [] as PublicLogicalModel[] };
}

export async function listAvailableLogicalModels(_intent: ModelRequestIntent) {
  return { models: [] as PublicLogicalModel[] };
}

export async function listAdminLogicalModels() {
  return { models: [] as AdminLogicalModel[] };
}

export async function createAdminLogicalModel(_input: LogicalModelMutation) {
  throw new Error('logical models are not available in allo canvas');
}

export async function updateAdminLogicalModel(_id: string, _input: LogicalModelMutation) {
  throw new Error('logical models are not available in allo canvas');
}

export async function deleteAdminLogicalModel(_id: string) {
  throw new Error('logical models are not available in allo canvas');
}

export async function simulateAdminLogicalModel(_id: string, _intent: ModelRequestIntent) {
  throw new Error('logical models are not available in allo canvas');
}
