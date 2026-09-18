export const reorderQueuedCommand = <T extends { id: string }>(
  items: T[],
  activeCommandId: string,
  overCommandId: string
): T[] => {
  const fromIndex = items.findIndex((item) => item.id === activeCommandId);
  const targetIndex = items.findIndex((item) => item.id === overCommandId);

  if (fromIndex === -1 || targetIndex === -1 || fromIndex === targetIndex) {
    return items;
  }

  const nextItems = [...items];
  const [movedItem] = nextItems.splice(fromIndex, 1);
  nextItems.splice(targetIndex, 0, movedItem);
  return nextItems;
};

export const promoteQueuedCommand = <T extends { id: string }>(items: T[], commandId: string): T[] => {
  const fromIndex = items.findIndex((item) => item.id === commandId);
  if (fromIndex <= 0) {
    return items;
  }

  const nextItems = [...items];
  const [movedItem] = nextItems.splice(fromIndex, 1);
  nextItems.unshift(movedItem);
  return nextItems;
};
