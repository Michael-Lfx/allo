/**
 * Collapse chrome for installed MCP rows.
 *
 * Arco renders the expand icon as `position:absolute; left:13px; top:50%`,
 * i.e. vertically centered on the whole (multi-row) header, and relies on a
 * 32px header padding-left to keep it clear of the title. Our headers are
 * taller than one line, so:
 * - pin the icon to the 24px title row (header pads 14px, so center = 26px);
 * - reserve a 40px indent on both header and content box so text lines up
 *   under the title instead of under the icon.
 *
 * Arco's header-title is `display: inline` with a 24px line box, so Latin
 * glyphs in a CJK UI font sit on a low baseline. Force a flex row and optically
 * lift the title onto the icon midline.
 */
export const MCP_SERVER_COLLAPSE_CLASS =
  'mb-4 [&_.arco-collapse-item-header]:!items-center [&_.arco-collapse-item-header]:!leading-none [&_.arco-collapse-item-header]:!py-14px [&_.arco-collapse-item-header]:!pl-40px [&_.arco-collapse-item-header]:!pr-16px [&_.arco-collapse-item-icon-hover]:!top-26px [&_.arco-collapse-item-header-title]:!flex [&_.arco-collapse-item-header-title]:!flex-1 [&_.arco-collapse-item-header-title]:!items-center [&_.arco-collapse-item-header-title]:!min-h-24px [&_.arco-collapse-item-header-title]:!min-w-0 [&_.arco-collapse-item-header-title]:!leading-none [&_.arco-collapse-item-content-box]:!pt-0 [&_.arco-collapse-item-content-box]:!pb-16px [&_.arco-collapse-item-content-box]:!pl-40px [&_.arco-collapse-item-content-box]:!pr-16px';

/** Server name next to 24px action icons. */
export const MCP_SERVER_TITLE_CLASS = 'inline-flex h-24px items-center leading-none -translate-y-2px';
