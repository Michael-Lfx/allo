/**
 * Static contract gate for Arco buttons that render an icon and visible text.
 *
 * This intentionally scans source rather than a hand-maintained list so a new
 * application button cannot silently skip the shared horizontal-layout class.
 * Third-party buttons are outside the scan because they do not import Arco's
 * Button from @arco-design/web-react.
 */

import { readdirSync, readFileSync } from 'node:fs';
import { extname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import tsModule from '../ui/node_modules/typescript/lib/typescript.js';

const ts = tsModule.default ?? tsModule;
const root = fileURLToPath(new URL('..', import.meta.url));
const rendererRoot = join(root, 'ui', 'src', 'renderer');
const contractClass = 'flowy-icon-text-btn';

const collectTsxFiles = (directory) => {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...collectTsxFiles(path));
    } else if (entry.isFile() && extname(entry.name) === '.tsx') {
      files.push(path);
    }
  }
  return files;
};

const hasVisibleChildren = (element) =>
  element.children.some((child) => {
    if (ts.isJsxText(child)) return child.getText().trim().length > 0;
    if (ts.isJsxExpression(child)) {
      if (!child.expression) return false;
      return !/^(?:null|undefined|false|true)$/.test(child.expression.getText().trim());
    }
    return true;
  });

const classExpressionIncludesContract = (expression, variables, seen = new Set()) => {
  if (!expression) return false;
  const text = expression.getText();
  if (text.includes(contractClass)) return true;
  if (!ts.isIdentifier(expression) || seen.has(expression.text)) return false;

  const initializer = variables.get(expression.text);
  if (!initializer) return false;
  seen.add(expression.text);
  return classExpressionIncludesContract(initializer, variables, seen);
};

const buttonClassIncludesContract = (element, variables) => {
  const className = element.attributes.properties.find(
    (attribute) => ts.isJsxAttribute(attribute) && attribute.name.text === 'className'
  );
  if (!className || !ts.isJsxAttribute(className) || !className.initializer) return false;
  if (ts.isStringLiteral(className.initializer) || ts.isNoSubstitutionTemplateLiteral(className.initializer)) {
    return className.initializer.text.includes(contractClass);
  }
  if (!ts.isJsxExpression(className.initializer)) return false;
  return classExpressionIncludesContract(className.initializer.expression, variables);
};

const inspectFile = (filePath) => {
  const source = readFileSync(filePath, 'utf8');
  const sourceFile = ts.createSourceFile(filePath, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const buttonNames = new Set();
  const variables = new Map();

  const collectVariables = (node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer) {
      variables.set(node.name.text, node.initializer);
    }
    ts.forEachChild(node, collectVariables);
  };

  sourceFile.statements.forEach((statement) => {
    if (
      ts.isImportDeclaration(statement) &&
      ts.isStringLiteral(statement.moduleSpecifier) &&
      statement.moduleSpecifier.text === '@arco-design/web-react'
    ) {
      const bindings = statement.importClause?.namedBindings;
      if (bindings && ts.isNamedImports(bindings)) {
        bindings.elements.forEach((element) => {
          if (element.propertyName?.text === 'Button' || element.name.text === 'Button') {
            buttonNames.add(element.name.text);
          }
        });
      }
    }

  });
  collectVariables(sourceFile);

  if (buttonNames.size === 0) return [];

  const violations = [];
  const visit = (node) => {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const opening = ts.isJsxElement(node) ? node.openingElement : node;
      const tagName = opening.tagName;
      if (ts.isIdentifier(tagName) && buttonNames.has(tagName.text)) {
        const iconAttribute = opening.attributes.properties.find(
          (attribute) => ts.isJsxAttribute(attribute) && attribute.name.text === 'icon'
        );
        const hasText = ts.isJsxElement(node) && hasVisibleChildren(node);
        const iconExpression = iconAttribute && ts.isJsxAttribute(iconAttribute) ? iconAttribute.initializer?.getText() : '';
        const iconIsEmpty = /^(?:null|undefined)$/.test(iconExpression?.replace(/[{}\s]/g, '') ?? '');
        if (iconAttribute && hasText && !iconIsEmpty && !buttonClassIncludesContract(opening, variables)) {
          const location = sourceFile.getLineAndCharacterOfPosition(node.getStart());
          violations.push({
            file: relative(root, filePath).replaceAll('\\', '/'),
            line: location.line + 1,
            source: node.getText(sourceFile).split('\n')[0].trim(),
          });
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return violations;
};

export const scanButtonLayoutContracts = () => collectTsxFiles(rendererRoot).flatMap(inspectFile);

if (resolve(process.argv[1] ?? '') === resolve(fileURLToPath(import.meta.url))) {
  const files = collectTsxFiles(rendererRoot);
  const violations = files.flatMap(inspectFile);
  if (violations.length > 0) {
    console.error(`[check:button-layout-contract] ${violations.length} Arco icon/text button(s) missing .${contractClass}:`);
    violations.forEach((violation) => console.error(`- ${violation.file}:${violation.line} ${violation.source}`));
    process.exit(1);
  }

  console.log(`[check:button-layout-contract] passed: scanned ${files.length} TSX files`);
}
