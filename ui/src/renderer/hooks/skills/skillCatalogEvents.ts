export const SKILL_CATALOG_CHANGED_EVENT = 'skill-catalog-changed';

export const notifySkillCatalogChanged = (): void => {
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new Event(SKILL_CATALOG_CHANGED_EVENT));
  }
};
