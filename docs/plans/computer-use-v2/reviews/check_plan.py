#!/usr/bin/env python3
"""只读检查 CU v2 计划的链接、任务卡、依赖、工时与评审处置覆盖。"""
from collections import Counter, defaultdict
from decimal import Decimal
from pathlib import Path
import re
import unicodedata
from urllib.parse import unquote, urlsplit

PLAN = Path(__file__).resolve().parent.parent
CARD = r'(?:P\d+-\d+[a-z]?|S-\d+)'
ISSUES = []


def fail(message):
    ISSUES.append(message)


def prose(text):
    """保留行号，排除代码块；代码里的 PR 模板路径相对仓库根而非本文。"""
    fence = None
    lines = []
    for line in text.splitlines():
        m = re.match(r'^\s*(`{3,}|~{3,})', line)
        if m:
            if fence is None:
                fence = m[1][0]
            elif m[1][0] == fence:
                fence = None
            lines.append('')
        else:
            lines.append(line if fence is None else '')
    return '\n'.join(lines)


def slug(title):
    title = re.sub(r'!?\[([^\]]*)\]\([^)]*\)', r'\1', title)
    title = title.lower().strip()
    # GitHub：空格转 -，保留字母/数字/中文/组合标记/_/-，去其他标点。
    return ''.join('-' if c == ' ' else c for c in title
                   if c == ' ' or c in '_-' or unicodedata.category(c)[0] in 'LNM')


def anchors(path):
    seen = Counter()
    result = set()
    for heading in re.findall(r'^#{1,6}\s+(.+?)\s*#*$', prose(path.read_text()), re.M):
        base = slug(heading)
        suffix = f'-{seen[base]}' if seen[base] else ''
        result.add(base + suffix)
        seen[base] += 1
    result.update(re.findall(r'<a\s+(?:id|name)=[\"\']([^\"\']+)', path.read_text()))
    return result


files = sorted(PLAN.rglob('*.md'))
frozen_sources = {
    '2026-10-13-security-threat-review.md',
    '2026-10-13-crates-verification.md',
    '2026-10-13-consistency-review.md',
}
links = 0
for path in files:
    text = prose(path.read_text())
    # inline code 中的 [text](url) 和评审引用示例不是实际 Markdown 链接。
    text = re.sub(r'(`+)(.*?)\1', lambda m: ' ' * len(m[0]), text)
    for m in re.finditer(r'!?\[[^\]\n]*\]\(([^)\n]+)\)', text):
        target = m[1].strip().split(' "', 1)[0].strip('<>')
        url = urlsplit(target)
        # 三份原文的机器绝对路径保留为历史证据；只检查可移植的相对链接。
        if url.scheme or url.netloc or target.startswith('/'):
            continue
        line = text[:m.start()].count('\n') + 1
        # 原评审的建议文字按被评审的计划目录书写；移入 reviews 后保留原文。
        base = PLAN if path.name in frozen_sources else path.parent
        dest = (base / unquote(url.path)).resolve() if url.path else path
        if not dest.exists():
            fail(f'{path.relative_to(PLAN)}:{line}: 文件不存在 {target}')
        elif url.fragment and dest.suffix == '.md' and unquote(url.fragment) not in anchors(dest):
            fail(f'{path.relative_to(PLAN)}:{line}: 锚点不存在 {target}')
        links += 1

roadmap = (PLAN / '06-roadmap.md').read_text()
blocks = re.split(rf'(?=^### {CARD} )', roadmap, flags=re.M)
cards = {}
for block in blocks:
    m = re.match(rf'### ({CARD}) ', block)
    if not m:
        continue
    ident = m[1]
    if ident in cards:
        fail(f'重复任务卡 {ident}')
    dm = re.search(r'^- \*\*依赖：\*\* (.*?)　\*\*估算：\*\* ([\d.]+)d', block, re.M)
    if not dm:
        fail(f'{ident}: 缺依赖/估算字段')
        continue
    field, days = dm.groups()
    if not re.fullmatch(rf'(?:—|{CARD}(?:、{CARD})*)', field):
        fail(f'{ident}: 依赖必须为纯卡号列表: {field}')
    deps = set(re.findall(CARD, field))
    cards[ident] = (deps, Decimal(days))
    for label in ('文件', '测试', 'DoD'):
        if f'- **{label}：**' not in block:
            fail(f'{ident}: 缺 {label}')
    acceptance = re.search(r'验收命令：(.*)', block)
    if not acceptance or not re.search(r'(?:r?cargo) (?:\S+ )*-p [\w-]+', acceptance[1]):
        fail(f'{ident}: 缺限定 -p 的 cargo 验收命令')
    if acceptance:
        test = block.index('- **测试：**')
        tail = block[test + len('- **测试：**'):]
        boundary = re.search(r'\n(?:- \*\*|---|## |\*\*P0 退出)', tail)
        end = test + len('- **测试：**') + boundary.start() if boundary else len(block)
        if not test < acceptance.start() < end:
            fail(f'{ident}: 验收命令不在测试小节末尾')

status = {}
for line in (PLAN / 'STATUS.md').read_text().splitlines():
    if re.match(rf'^\| {CARD} \|', line):
        cols = [v.strip() for v in line.strip('|').split('|')]
        ident, _, deps, days = cols[:4]
        if ident in status:
            fail(f'STATUS 重复卡 {ident}')
        status[ident] = (set(re.findall(CARD, deps)), Decimal(days.rstrip('d')))
if set(cards) != set(status):
    fail(f'STATUS/06 卡不对应: {set(cards) ^ set(status)}')
for ident, (deps, days) in cards.items():
    if ident in status and status[ident] != (deps, days):
        fail(f'{ident}: STATUS 依赖或工时与 06 不一致')
    for dependency in deps:
        if dependency not in cards:
            fail(f'{ident}: 悬空依赖 {dependency}')
    if ident in deps:
        fail(f'{ident}: 自依赖')

visiting, visited = set(), set()


def visit(ident):
    if ident in visiting:
        fail(f'依赖环: {ident}')
        return
    if ident in visited or ident not in cards:
        return
    visiting.add(ident)
    for dependency in cards[ident][0]:
        visit(dependency)
    visiting.remove(ident)
    visited.add(ident)


for ident in cards:
    visit(ident)
# 冻结评审中的建议/示例卡引用不代表现行任务依赖，不用于检查现行悬空引用。
for path in [*PLAN.glob('*.md'), PLAN / 'reviews/2026-10-13-disposition.md']:
    for ident in re.findall(rf'(?<![\w]){CARD}(?![\w])', path.read_text()):
        if ident not in cards:
            fail(f'{path.name}: 悬空卡号 {ident}')

sums = defaultdict(Decimal)
for ident, (_, days) in cards.items():
    sums[ident.split('-')[0]] += days
for phase, goal, exit_criteria, days in re.findall(r'^\| (P\d+|Spike) \| (.*?) \| (.*?) \| ([\d.]+)d \|$', roadmap, re.M):
    key = 'S' if phase == 'Spike' else phase
    if sums[key] != Decimal(days):
        fail(f'{phase}: 总览 {days}d != 卡片 {sums[key]}d')

reviews = PLAN / 'reviews'
security = len(re.findall(r'^\d+\. \[严重度', (reviews / '2026-10-13-security-threat-review.md').read_text(), re.M))
crates = len(re.findall(r'^\[严重度', (reviews / '2026-10-13-crates-verification.md').read_text(), re.M))
consistency = len(re.findall(r'^\[严重度', (reviews / '2026-10-13-consistency-review.md').read_text(), re.M))
expected = {'安全': security, '依赖': crates, '一致性': consistency}
rows = defaultdict(list)
for category, number, decision, position in re.findall(r'^\| (安全|依赖|一致性) \| (\d+)\.[^|]* \| ([^|]*) \| ([^|]*) \|$', (reviews / '2026-10-13-disposition.md').read_text(), re.M):
    rows[category].append(int(number))
    if decision.strip() not in ('采纳', '修改后采纳', '不采纳') or not position.strip():
        fail(f'{category} {number}: 无有效处置/改动位置或理由')
for category, count in expected.items():
    if sorted(rows[category]) != list(range(1, count + 1)):
        fail(f'{category}: 处置编号不连续/有遗漏，应为1..{count}')

if ISSUES:
    print('\n'.join(ISSUES))
    raise SystemExit(1)
print(f'PASS: {len(files)}份Markdown，{links}个相对链接/锚点；{len(cards)}张卡与STATUS一一对应，依赖纯卡号/无悬空/无环，文件/测试/DoD/验收命令齐全。')
print('PASS: 阶段工时 ' + '、'.join(f'{key}={value}d' for key, value in sorted(sums.items())) + '；P0–P9=' + str(sum(value for key, value in sums.items() if key.startswith('P'))) + 'd，spike=' + str(sums['S']) + 'd。')
print(f'PASS: 处置 {security}+{crates}+{consistency}={sum(expected.values())} 条，无遗漏或重复。')
