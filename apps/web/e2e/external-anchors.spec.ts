import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import ts from 'typescript';
import { canonicalOrigin } from '../src/lib/app-origin';
import { isBackendHref } from '../src/lib/backend-href';

// Resolve literals, finite unions (e.g. the main nav), and ordinary dynamic hrefs.
// Unknown values fail closed unless the anchor already opts out of routing.
function hrefs(expression: ts.Expression, checker?: ts.TypeChecker): string[] | undefined {
  if (ts.isStringLiteralLike(expression)) return [expression.text];
  if (ts.isParenthesizedExpression(expression)) return hrefs(expression.expression, checker);
  if (ts.isConditionalExpression(expression)) {
    const yes = hrefs(expression.whenTrue, checker);
    const no = hrefs(expression.whenFalse, checker);
    if (yes && no) return [...yes, ...no];
  }
  if (ts.isBinaryExpression(expression) && expression.operatorToken.kind === ts.SyntaxKind.PlusToken) {
    const starts = hrefs(expression.left, checker);
    const ends = hrefs(expression.right, checker);
    if (starts && ends) return starts.flatMap(start => ends.map(end => start + end));
    // Unresolved data appended after a query or fragment cannot change the path.
    if (starts?.every(start => /^[^?#]*[?#]/.test(start))) return starts;
  }
  if (ts.isTemplateExpression(expression) && /[?#]/.test(expression.head.text)) {
    return [expression.head.text];
  }
  const type = checker?.getTypeAtLocation(expression);
  const members = type?.isUnion() ? type.types : type ? [type] : [];
  if (members.length && members.every(member => member.isStringLiteral())) return members.map(member => member.value);
}

function violations(file: ts.SourceFile, checker?: ts.TypeChecker) {
  const results: string[] = [];
  const opaque = Symbol('value may be set by a JSX spread');
  function visit(node: ts.Node) {
    if ((ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) &&
      ts.isIdentifier(node.tagName) && ['a', 'A', 'CampaignLink'].includes(node.tagName.text)) {
      // The wrapper opts backend links out at runtime (including computed hrefs).
      // Its consumers still undergo the same static check as ordinary anchors.
      if (node.tagName.text === 'A' && file.fileName.endsWith('/src/lib/attribution-link.tsx')) return;
      let href: ts.JsxAttribute | typeof opaque | undefined;
      let rel: ts.JsxAttribute | typeof opaque | undefined;
      for (const attr of node.attributes.properties) {
        if (ts.isJsxSpreadAttribute(attr)) href = rel = opaque;
        else if (ts.isIdentifier(attr.name) && attr.name.text === 'href') href = attr;
        else if (ts.isIdentifier(attr.name) && attr.name.text === 'rel') rel = attr;
      }
      const relValue = rel && rel !== opaque ? rel.initializer : undefined;
      const relText = relValue && (ts.isStringLiteral(relValue) ? relValue.text :
        ts.isJsxExpression(relValue) && relValue.expression && ts.isStringLiteralLike(relValue.expression) ? relValue.expression.text : '');
      if (href && !relText?.split(/[\t\n\f\r ]+/).includes('external')) {
        const value = href === opaque ? undefined : href.initializer;
        const targets = value && (ts.isStringLiteral(value) ? [value.text] :
          ts.isJsxExpression(value) && value.expression ? hrefs(value.expression, checker) : undefined);
        if (!targets || targets.some(target => /&(?:#|[a-z][a-z\d]*;)/i.test(target) || isBackendHref(target, canonicalOrigin))) {
          const { line, character } = file.getLineAndCharacterOfPosition(node.getStart(file));
          results.push(`${file.fileName}:${line + 1}:${character + 1} — add rel="external" or use a statically known page href`);
        }
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(file);
  return results;
}

test('backend and unresolved anchors opt out of Solid Router navigation', () => {
  const app = fileURLToPath(new URL('..', import.meta.url));
  const config = ts.readConfigFile(app + '/tsconfig.json', ts.sys.readFile);
  expect(config.error).toBeUndefined();
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, app);
  // Scan all app JSX, even if a new file is not listed in tsconfig's include yet.
  const files = ts.sys.readDirectory(app + '/src', ['.tsx', '.jsx'], ['**/node_modules/**']);
  expect(files.length).toBeGreaterThan(0);
  const program = ts.createProgram(files, { ...parsed.options, allowJs: true });
  const checker = program.getTypeChecker();
  expect(files.flatMap(file => violations(program.getSourceFile(file)!, checker))).toEqual([]);
});

test('anchor guard catches dynamic links, near-miss rel values and opaque props', () => {
  const fixture = ts.createSourceFile('fixture.tsx', [
    '<a href="/api/download">Download</a>;',
    '<a href={"/auth/login" + query()} rel="notexternal">Login</a>;',
    '<A href={loggedIn ? "/dashboard" : "/oauth/start"}>Continue</A>;',
    '<a href={`/oauth/start?next=${next}`} />;',
    '<A href="https://anymd.sylphx.com/api/health" />;',
    '<A href={computed()} />;',
    '<a href="/pricing" rel="external" {...attrs} />;',
    '<a href={"/api/auth/google" + query()} rel="nofollow external" />;',
    '<a {...attrs} rel={"external"} />;',
    '<A href={"/sign-in?next=" + encodeURIComponent(next)} />;',
  ].join('\n'), ts.ScriptTarget.Latest, true);
  expect(violations(fixture)).toEqual([1, 2, 3, 4, 5, 6, 7].map(line => 'fixture.tsx:' + line + ':1 — add rel="external" or use a statically known page href'));
});
