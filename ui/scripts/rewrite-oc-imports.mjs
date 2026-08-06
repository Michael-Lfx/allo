import fs from 'node:fs';
import path from 'node:path';

const root = process.argv[2];
if (!root) {
  console.error('usage: rewrite-oc-imports.mjs <dir>');
  process.exit(1);
}

function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full);
      continue;
    }
    if (!/\.(ts|tsx)$/.test(entry.name)) continue;
    let content = fs.readFileSync(full, 'utf8');
    const next = content
      .replaceAll('from "@/', 'from "@oc/')
      .replaceAll("from '@/", "from '@oc/")
      .replaceAll('import("@/', 'import("@oc/')
      .replaceAll("import('@/", "import('@oc/")
      // Restore allo root aliases that must NOT live under @oc
      .replaceAll('from "@oc/common/', 'from "@/common/')
      .replaceAll("from '@oc/common/", "from '@/common/")
      .replaceAll('from "@oc/renderer/', 'from "@renderer/')
      .replaceAll("from '@oc/renderer/", "from '@renderer/")
      .replaceAll('import("@oc/common/', 'import("@/common/')
      .replaceAll("import('@oc/common/", "import('@/common/")
      .replaceAll('import("@oc/renderer/', 'import("@renderer/')
      .replaceAll("import('@oc/renderer/", "import('@renderer/")
      .replaceAll('from "react-router"', 'from "react-router-dom"')
      .replaceAll("from 'react-router'", "from 'react-router-dom'")
      .replaceAll('navigate("/canvas"', 'navigate("/video-generation/canvas"')
      .replaceAll("navigate('/canvas'", "navigate('/video-generation/canvas'")
      .replaceAll('to="/canvas"', 'to="/video-generation/canvas"')
      .replaceAll("to='/canvas'", "to='/video-generation/canvas'");
    if (next !== content) fs.writeFileSync(full, next, 'utf8');
  }
}

walk(root);
console.log('rewrote imports under', root);
