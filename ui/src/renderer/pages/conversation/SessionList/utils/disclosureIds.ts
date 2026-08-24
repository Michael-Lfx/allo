import { workpathKey } from './workpathKey';

/**
 * Encode dynamic disclosure id parts without lossy character replacement.
 * `encodeURIComponent` is injective for strings and produces an id-safe value
 * without whitespace, so two distinct workpaths cannot collapse to one id.
 */
const encodeIdPart = (value: string): string => encodeURIComponent(value);

export type WorkpathDisclosureIds = {
  sessionsId: string;
  overflowId: string;
};

export const getWorkpathDisclosureIds = (workpath: string, instanceId: string): WorkpathDisclosureIds => {
  const workpathPart = encodeIdPart(workpathKey(workpath));
  const instancePart = encodeIdPart(instanceId);
  const prefix = `flowy-workpath-${instancePart}-${workpathPart}`;

  return {
    sessionsId: `${prefix}-sessions`,
    overflowId: `${prefix}-overflow`,
  };
};

export const getCompanionDisclosureIds = (instanceId: string): WorkpathDisclosureIds => {
  const prefix = `flowy-companion-${encodeIdPart(instanceId)}`;

  return {
    sessionsId: `${prefix}-sessions`,
    overflowId: `${prefix}-overflow`,
  };
};
