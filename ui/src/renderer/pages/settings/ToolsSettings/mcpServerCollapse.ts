/**
 * Collapse chrome for installed MCP rows.
 *
 * Arco's header-title is `display: inline` with a 24px line box, so Latin
 * glyphs in a CJK UI font sit on a low baseline. Force a flex row and optically
 * lift the title onto the icon midline.
 */
export const MCP_SERVER_COLLAPSE_CLASS =
  'mb-4 [&_.arco-collapse-item-header]:!items-center [&_.arco-collapse-item-header]:!leading-none [&_.arco-collapse-item-header-title]:!flex [&_.arco-collapse-item-header-title]:!flex-1 [&_.arco-collapse-item-header-title]:!items-center [&_.arco-collapse-item-header-title]:!min-h-24px [&_.arco-collapse-item-header-title]:!min-w-0 [&_.arco-collapse-item-header-title]:!leading-none';

/** Server name next to 24px action icons. */
export const MCP_SERVER_TITLE_CLASS = 'inline-flex h-24px items-center leading-none -translate-y-2px';
