/** Fired when titlebar search opens a conversation, so the sider list returns to projects. */
export const SIDER_HISTORY_SEARCH_SELECT_EVENT = 'flowy:sider-history-search-select';

export const dispatchSiderHistorySearchSelect = (): void => {
  window.dispatchEvent(new Event(SIDER_HISTORY_SEARCH_SELECT_EVENT));
};
