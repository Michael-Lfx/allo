export type ApprovalKind = 'edit' | 'exec' | 'info' | 'mcp' | 'plan';

export const kindFromConfirmationType = (type: 'edit' | 'exec' | 'info' | 'mcp'): ApprovalKind => type;

export const kindFromPermissionAction = (action: string | undefined): ApprovalKind => {
  switch (action) {
    case 'edit':
    case 'exec':
    case 'info':
    case 'mcp':
      return action;
    default:
      return 'info';
  }
};
