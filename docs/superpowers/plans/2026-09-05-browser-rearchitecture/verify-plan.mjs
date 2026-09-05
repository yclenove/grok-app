import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';

const planDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(planDir, '../../../..');
const specDir = path.join(root, 'docs/superpowers/specs/2026-09-05-browser-rearchitecture');
const files = [
  ['m0-preview', 'M0-W'], ['m0-runtime', 'M0-R'],
  ['m1-gateway', 'M1-G'], ['m1-runtime', 'M1-R'], ['m1-delivery', 'M1-D'],
  ['m2-inspection', 'M2-'], ['m3-chrome', 'M3-'], ['m4-linux', 'M4-'],
  ['m5-ssh', 'M5-'], ['m6-updates', 'M6-'],
];
const entryGates = {
  'm0-preview': ['M0-W01'], 'm0-runtime': ['M0-W01'],
  'm1-gateway': ['M0-W06', 'M0-R06'], 'm1-runtime': ['M0-R06', 'M0-W01'],
  'm1-delivery': ['M0-R06', 'M1-R01'],
  'm2-inspection': ['M1-G06', 'M1-R06', 'M1-D06'],
  'm3-chrome': ['M1-G06', 'M1-R06', 'M1-D06'],
  'm4-linux': ['M1-G06', 'M1-R06', 'M1-D06'],
  'm5-ssh': ['M1-G06', 'M1-R06', 'M1-D06'],
  'm6-updates': ['M1-R06', 'M1-D06'],
};
const args = new Set(process.argv.slice(2));
assert([...args].every((arg) => arg === '--write-index'), 'Only --write-index is supported');

// Existing linked worktrees can share the main checkout's installed Markdown parser.
const commonDir = execFileSync('git', ['rev-parse', '--git-common-dir'], {
  cwd: root, encoding: 'utf8',
}).trim();
const dependencyRoots = [root, path.dirname(path.resolve(root, commonDir))];
let parserRequire;
let gfmPath;
for (const candidate of dependencyRoots) {
  try {
    const require = createRequire(path.join(candidate, 'package.json'));
    parserRequire = createRequire(require.resolve('react-markdown'));
    gfmPath = require.resolve('remark-gfm');
    break;
  } catch {
    parserRequire = undefined;
  }
}
assert(parserRequire, 'Install existing repo dependencies with pnpm install --frozen-lockfile');
const { unified } = await import(pathToFileURL(parserRequire.resolve('unified')));
const { default: remarkParse } = await import(pathToFileURL(parserRequire.resolve('remark-parse')));
const { default: remarkGfm } = await import(pathToFileURL(gfmPath));
const processor = unified().use(remarkParse).use(remarkGfm);
const taskPattern = /\bM[0-6]-(?:[WGRD])?\d{2}\b/g;
const taskIdPattern = /^M[0-6]-(?:[WGRD])?\d{2}$/;
const estimatePattern = /(\d+(?:\.\d+)?)(?:\s*(?:-|–|\.\.|至)\s*)(\d+(?:\.\d+)?)/;
const issues = [];
const parsed = new Map();
const tasks = [];
const textOf = (node) => node.value ?? (node.children ?? []).map(textOf).join('');
const walk = (node, fn) => { fn(node); for (const child of node.children ?? []) walk(child, fn); };
const relative = (file) => path.relative(root, file).split(path.sep).join('/');
const fail = (file, message) => issues.push(`${relative(file)}: ${message}`);

function readMarkdown(file) {
  if (parsed.has(file)) return parsed.get(file);
  assert(fs.existsSync(file), `Missing required document: ${relative(file)}`);
  const source = fs.readFileSync(file, 'utf8');
  const ast = processor.parse(source);
  const stats = { headings: 0, tables: 0, codeBlocks: 0, links: 0 };
  const headingLabels = new Set();
  let depth = 0;
  walk(ast, (node) => {
    if (node.type === 'heading') {
      stats.headings++;
      const label = textOf(node);
      if (headingLabels.has(label)) fail(file, `duplicate heading ${label}`);
      headingLabels.add(label);
      if (node.depth > depth + 1) fail(file, `heading level skipped at line ${node.position.start.line}`);
      depth = node.depth;
    }
    if (node.type === 'table') {
      stats.tables++;
      const width = node.children[0].children.length;
      for (const row of node.children) if (row.children.length !== width) fail(file, `table width at line ${row.position.start.line}`);
    }
    if (node.type === 'code') {
      stats.codeBlocks++;
      if (node.lang === 'json') {
        try { JSON.parse(node.value); }
        catch { fail(file, `invalid JSON example at line ${node.position.start.line}`); }
      }
    }
    if (node.type === 'link') {
      stats.links++;
      if (/^https?:/.test(node.url)) new URL(node.url);
      else if (!node.url.startsWith('#')) {
        const target = path.resolve(path.dirname(file), decodeURIComponent(node.url.split('#')[0]));
        const generatedIndex = path.join(planDir, 'task-index.json');
        if (!fs.existsSync(target) && !(args.has('--write-index') && target === generatedIndex)) fail(file, `broken relative link ${node.url}`);
      }
    }
  });
  if (/\b(?:TODO|TBD|FIXME|PLACEHOLDER)\b|待定|后续补充|补充细节/.test(source)) fail(file, 'unresolved placeholder');
  if (!source.endsWith('\n')) fail(file, 'missing final newline');
  if (source.split('\n').filter((line) => /^```/.test(line)).length % 2) fail(file, 'unpaired code fence');
  const result = { source, ast, stats };
  parsed.set(file, result);
  return result;
}

for (const [name, prefix] of files) {
  const specPath = path.join(specDir, `${name}.md`);
  const planPath = path.join(planDir, `${name}.md`);
  const spec = readMarkdown(specPath);
  const plan = readMarkdown(planPath);
  if (spec.stats.headings < 6) fail(specPath, 'spec must define multiple concrete design sections');
  const headings = plan.ast.children.filter((node) => node.type === 'heading' && node.depth === 3 && /^M[0-6]-[WGRD]?\d{2}[：:]/.test(textOf(node)));
  if (headings.length !== 6) fail(planPath, `expected 6 tasks, found ${headings.length}`);
  for (let i = 0; i < headings.length; i++) {
    const heading = headings[i];
    const label = textOf(heading);
    const id = label.match(taskPattern)[0];
    const expectedId = `${prefix}${String(i + 1).padStart(2, '0')}`;
    if (id !== expectedId) fail(planPath, `expected ${expectedId}, found ${id}`);
    const start = heading.position.start.offset;
    const nextSection = plan.ast.children.find((node) =>
      node.type === 'heading' && node.depth <= heading.depth && node.position.start.offset > start);
    const end = nextSection?.position.start.offset ?? plan.source.length;
    const section = plan.source.slice(start, end);
    const sectionAst = processor.parse(section);
    const field = (name) => section.match(new RegExp(`\\*\\*${name}：\\*\\*([^\\n]*)`))?.[1].trim();
    const dependencyText = field('依赖');
    const owner = field('负责');
    const estimate = field('估算');
    const estimateMatch = estimate?.match(estimatePattern);
    for (const fieldName of ['依赖', '负责', '估算', '文件']) if (field(fieldName) === undefined) fail(planPath, `${id} missing ${fieldName}`);
    if (!owner) fail(planPath, `${id} missing owner`);
    if (!estimateMatch) fail(planPath, `${id} estimate must include explicit range`);
    if (estimateMatch && (+estimateMatch[1] <= 0 || +estimateMatch[1] > +estimateMatch[2])) fail(planPath, `${id} invalid estimate range`);
    const dependencyBody = (dependencyText ?? '').replaceAll('`', '').replace(/（[^）]*）/g, '').trim().replace(/[。.]+$/, '');
    const dependencies = dependencyBody === '无' ? [] : dependencyBody.split(/[、,，]/).map((dep) => dep.trim());
    if (!dependencies.length && id !== 'M0-W01') fail(planPath, `${id} cannot omit all dependencies`);
    for (const dep of dependencies) if (!taskIdPattern.test(dep)) fail(planPath, `${id} malformed dependency ${dep}`);
    if (new Set(dependencies).size !== dependencies.length) fail(planPath, `${id} duplicate dependencies`);
    let steps = 0;
    let completedSteps = 0;
    let codeBlocks = 0;
    walk(sectionAst, (node) => {
      if (node.type === 'listItem' && typeof node.checked === 'boolean') { steps++; if (node.checked) completedSteps++; }
      if (node.type === 'code') codeBlocks++;
    });
    if (steps !== 6) fail(planPath, `${id} needs exactly 6 executable steps, found ${steps}`);
    if (!codeBlocks) fail(planPath, `${id} missing concrete code/fixture example`);
    if (!/\b(?:cargo|pnpm|node|python3)\b/.test(section)) fail(planPath, `${id} missing verification command`);
    if (!/验收|退出条件/.test(section)) fail(planPath, `${id} missing acceptance`);
    tasks.push({
      id, title: label.slice(id.length).replace(/^[：:]\s*/, ''), package: name,
      plan: relative(planPath), spec: relative(specPath), line: heading.position.start.line,
      dependencies, owner,
      estimateDays: estimateMatch ? { min: Number(estimateMatch[1]), max: Number(estimateMatch[2]) } : null,
      steps, completedSteps,
      status: completedSteps === 0 ? 'not_started' : completedSteps === steps ? 'implementation_recorded' : 'in_progress',
    });
  }
}

for (const file of ['00-contracts.md']) readMarkdown(path.join(specDir, file));
for (const file of ['README.md', 'acceptance-matrix.md']) readMarkdown(path.join(planDir, file));
readMarkdown(path.join(root, 'docs/superpowers/specs/2026-09-04-browser-rearchitecture-design.md'));
readMarkdown(path.join(root, 'docs/plans/2026-09-05-browser-rearchitecture-HANDOFF.md'));
const byId = new Map(tasks.map((task) => [task.id, task]));
assert.equal(byId.size, tasks.length, 'Duplicate task IDs');
assert.equal(tasks.length, 60, 'Expected the full 60-task program');
for (const task of tasks) for (const dep of task.dependencies) if (!byId.has(dep)) issues.push(`${task.id}: missing dependency ${dep}`);
const visiting = new Set();
const depths = new Map();
function level(id) {
  if (depths.has(id)) return depths.get(id);
  assert(!visiting.has(id), `Dependency cycle at ${id}`);
  visiting.add(id);
  const task = byId.get(id);
  if (!task) { visiting.delete(id); return 0; }
  const depth = Math.max(0, ...task.dependencies.map((dep) => level(dep) + 1));
  visiting.delete(id);
  depths.set(id, depth);
  return depth;
}
for (const task of tasks) task.dependencyLevel = level(task.id);
const ancestors = new Map();
function ancestorIds(id) {
  if (ancestors.has(id)) return ancestors.get(id);
  const result = new Set();
  for (const dep of byId.get(id)?.dependencies ?? []) {
    result.add(dep);
    for (const parent of ancestorIds(dep)) result.add(parent);
  }
  ancestors.set(id, result);
  return result;
}
for (const task of tasks) {
  for (const gate of entryGates[task.package]) {
    if (gate !== task.id && !ancestorIds(task.id).has(gate)) issues.push(`${task.id}: required entry gate ${gate} is not reachable`);
  }
}

const matrixFile = path.join(planDir, 'acceptance-matrix.md');
const matrix = parsed.get(matrixFile);
const requirements = [];
walk(matrix.ast, (node) => {
  if (node.type !== 'tableRow') return;
  const cells = node.children.map(textOf);
  if (!/^BR-\d{2}$/.test(cells[0])) return;
  requirements.push({ id: cells[0], outcome: cells[1], tasks: [...new Set(cells[2].match(taskPattern) ?? [])], evidence: cells[3] });
});
assert.equal(requirements.length, 24, 'Expected 24 mapped requirements');
for (let i = 1; i <= 24; i++) assert(requirements.some((req) => req.id === `BR-${String(i).padStart(2, '0')}`), `Missing BR-${i}`);
for (const req of requirements) {
  if (!req.tasks.length) fail(matrixFile, `${req.id} has no tasks`);
  for (const id of req.tasks) if (!byId.has(id)) fail(matrixFile, `${req.id} references missing ${id}`);
}
for (const task of tasks) if (!requirements.some((req) => req.tasks.includes(task.id))) fail(matrixFile, `unmapped task ${task.id}`);
for (const [file, doc] of parsed) {
  for (const id of new Set(doc.source.match(taskPattern) ?? [])) if (!byId.has(id)) fail(file, `unknown task reference ${id}`);
}
const sumEstimates = (members) => members.reduce((sum, task) => ({ min: sum.min + (task.estimateDays?.min ?? 0), max: sum.max + (task.estimateDays?.max ?? 0) }), { min: 0, max: 0 });
const estimate = sumEstimates(tasks);
const packages = files.map(([name, prefix]) => {
  const members = tasks.filter((task) => task.package === name);
  return { id: name, label: prefix.replace(/-$/, ''), entryGates: entryGates[name], taskIds: members.map((task) => task.id), estimateDays: sumEstimates(members) };
});
const readmePath = path.join(planDir, 'README.md');
const effortRows = new Map();
walk(parsed.get(readmePath).ast, (node) => {
  if (node.type !== 'table' || textOf(node.children[0].children.at(-1)) !== '主动工程日') return;
  for (const row of node.children.slice(1)) {
    const cells = row.children.map(textOf);
    if (effortRows.has(cells[0])) fail(readmePath, `duplicate effort row ${cells[0]}`);
    effortRows.set(cells[0], cells.at(-1));
  }
});
for (const [label, expectedRange] of [...packages.map((pkg) => [pkg.label, pkg.estimateDays]), ['合计', estimate]]) {
  const actual = effortRows.get(label)?.match(estimatePattern);
  if (!actual || +actual[1] !== expectedRange.min || +actual[2] !== expectedRange.max) fail(readmePath, `${label} effort summary must be ${expectedRange.min}–${expectedRange.max}`);
}
assert.equal(issues.length, 0, issues.join('\n'));
const index = {
  schemaVersion: 1, objective: '浏览器重构 M0–M6 全套设计与开发计划',
  taskSource: 'Markdown 计划；使用 verify-plan.mjs --write-index 重新生成',
  verificationScope: '仅验证文档结构与可追踪性；产品验收必须提供实际运行证据',
  taskCount: tasks.length, requirementCount: requirements.length, estimateEngineeringDays: estimate,
  packages,
  tasks, requirements,
};
const indexPath = path.join(planDir, 'task-index.json');
const expected = `${JSON.stringify(index, null, 2)}\n`;
if (args.has('--write-index')) fs.writeFileSync(indexPath, expected);
else assert.equal(fs.readFileSync(indexPath, 'utf8'), expected, 'task-index.json is stale; regenerate --write-index');
console.log(JSON.stringify({ status: 'pass', documents: parsed.size, tasks: tasks.length, requirements: requirements.length, steps: tasks.reduce((n, task) => n + task.steps, 0), dependencyLevels: Math.max(...depths.values()) + 1, estimateEngineeringDays: estimate, productTestsRun: false }, null, 2));
