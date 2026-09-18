export type { AddNodeMenuCommand, AddNodeMenuContext, ResolvedAddNodeMenuCommand, ToolbarHandlers, ToolbarId, ToolbarPrefs, ToolCategory, ToolContext, ToolDefinition } from "./tool-definition";
export { clearToolbarPrefs, persistToolbarPrefs, readToolbarPrefs } from "./tool-persistence";
export { defaultToolbarPrefs, getAddNodeMenuCommands, getToolbarTools, registerAddNodeMenuCommands, registerToolbarTools, resolveAddNodeMenuCommands, resolveToolbarEntries, resolveToolbarTools } from "./tool-registry";

import { addNodeMenuCommands } from "./definitions/add-node-menu-tools";
import { mainToolbarTools } from "./definitions/main-toolbar-tools";
import { nodeHoverToolbarTools } from "./definitions/node-hover-tools";
import { selectionToolbarTools } from "./definitions/selection-toolbar-tools";
import { registerAddNodeMenuCommands, registerToolbarTools } from "./tool-registry";

// 定义文件只导出数组，由这里在模块求值完成后注册，避免
// `definitions → ../tool-registry` 解析到本 index 形成循环依赖，导致 register 未初始化、工具栏只剩空壳。
registerToolbarTools(mainToolbarTools);
registerToolbarTools(selectionToolbarTools);
registerToolbarTools(nodeHoverToolbarTools);
registerAddNodeMenuCommands(addNodeMenuCommands);
