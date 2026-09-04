/** Owner-scoped App Server workspace client (list + user-path registration). */

import type { Transport } from "./transport";
import type { WorkspaceRevokeResult, WorkspaceView } from "@agent-store/protocol";

export class WorkspaceClient {
  constructor(private readonly transport: Transport) {}

  list(): Promise<WorkspaceView[]> {
    return this.transport.request<WorkspaceView[]>("workspace/list", {});
  }

  /**
   * Validate + register (idempotently per canonical root) an absolute local
   * directory chosen by the owner. The server canonicalizes the path and
   * rejects links/reparse points; the returned view only carries the
   * canonical path back to this same authenticated connection.
   */
  create(path: string): Promise<WorkspaceView> {
    return this.transport.request<WorkspaceView>("workspace/create", { path });
  }

  /**
   * Revoke (soft-delete) an owner's active workspace. Its conversations are
   * kept and their `workspace_id` reference is preserved, so re-registering
   * the same path re-activates the same workspace and those conversations
   * become visible again.
   */
  revoke(workspaceId: string): Promise<WorkspaceRevokeResult> {
    return this.transport.request<WorkspaceRevokeResult>("workspace/revoke", {
      workspace_id: workspaceId,
    });
  }
}