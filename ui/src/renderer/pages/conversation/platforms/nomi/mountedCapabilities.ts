import type { IConversationMcpStatus, IConversationMcpStatusKind } from '@/common/config/storage';

export interface MountedMcpChip {
  id: string;
  name: string;
  status?: IConversationMcpStatusKind;
  reason?: string;
}

export interface MountedCapabilities {
  skills: string[];
  mcp: MountedMcpChip[];
}

export interface MountedSkillLabel {
  label: string;
  description?: string;
}

interface ConversationLike {
  extra?: unknown;
}

interface MountedCapabilitiesExtra {
  skills?: unknown;
  mcp_statuses?: unknown;
  mcp_servers?: unknown;
}

interface SkillCatalogLike {
  skillId: string;
  name: string;
  description?: string;
}

const MCP_STATUS_KINDS = new Set<IConversationMcpStatusKind>(['loaded', 'failed', 'unsupported']);

const readStringArray = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string' && item.length > 0) : [];

const readMcpStatuses = (value: unknown): MountedMcpChip[] => {
  if (!Array.isArray(value)) return [];
  const chips: MountedMcpChip[] = [];
  for (const item of value) {
    if (!item || typeof item !== 'object') continue;
    const statusItem = item as Partial<IConversationMcpStatus>;
    if (typeof statusItem.mcp_server_id !== 'string' || typeof statusItem.name !== 'string') continue;
    if (typeof statusItem.status !== 'string' || !MCP_STATUS_KINDS.has(statusItem.status)) continue;
    chips.push({
      id: statusItem.mcp_server_id,
      name: statusItem.name,
      status: statusItem.status,
      ...(typeof statusItem.reason === 'string' && statusItem.reason.length > 0 ? { reason: statusItem.reason } : {}),
    });
  }
  return chips;
};

export const getMountedCapabilities = (conversation: ConversationLike): MountedCapabilities => {
  const extra = (conversation.extra ?? {}) as MountedCapabilitiesExtra;
  const statuses = readMcpStatuses(extra.mcp_statuses);
  const mcp =
    statuses.length > 0
      ? statuses
      : readStringArray(extra.mcp_servers).map((name) => ({ id: name, name }));
  return {
    skills: readStringArray(extra.skills),
    mcp,
  };
};

export const hasMountedCapabilities = (mounted: MountedCapabilities): boolean =>
  mounted.skills.length > 0 || mounted.mcp.length > 0;

export const resolveMountedSkillLabel = (skillId: string, catalog: SkillCatalogLike[]): MountedSkillLabel => {
  const match = catalog.find((entry) => entry.skillId === skillId || entry.name === skillId);
  if (!match) return { label: skillId };
  return {
    label: match.name,
    ...(match.description ? { description: match.description } : {}),
  };
};
