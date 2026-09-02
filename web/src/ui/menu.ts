/**
 * Which conversation popover menu is open.
 *
 * Discriminated by *place* rather than keyed by conversation id on purpose:
 * a single `conversation_id` key would render BOTH menus when the selected
 * conversation also appears in the sidebar list. The sidebar variant carries
 * an anchor because that menu is portalled to `document.body` and positioned
 * at the clicked row; the topbar variant is anchored by CSS.
 */
export type OpenMenu =
  | { where: "sidebar"; id: string; anchor: { x: number; y: number } }
  | { where: "topbar" }
  | null;
