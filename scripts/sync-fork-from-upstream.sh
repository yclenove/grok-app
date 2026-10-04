#!/usr/bin/env bash
# Merge the GitHub parent repo's default branch into this fork's main.
#
# Usage (full checkout of main):
#   bash scripts/sync-fork-from-upstream.sh
#
# Env:
#   GH_TOKEN / GITHUB_TOKEN   gh + git push
#   DRY_RUN=1                 fetch only; no merge push / PR / Issue
#   UPSTREAM                  owner/name override (default: this repo's parent)
#
# Actions secret SYNC_UPSTREAM_TOKEN (recommended):
#   Fine-grained PAT on this fork, owner = fork owner:
#     Contents: Read and write
#     Pull requests: Read and write
#     Issues: Read and write
#     Workflows: Read and write
#       ← GitHub rejects GITHUB_TOKEN pushes that touch .github/workflows/*
#   Classic PAT: repo + workflow
#   Repo setting: Actions → General → Allow GitHub Actions to create and
#   approve pull requests (needed to open a conflict PR with GITHUB_TOKEN).
#
# This script never creates a git tag, so it cannot start release.yml.
set -euo pipefail

if [[ -z "${DRY_RUN:-}" && -n "$(git status --porcelain)" ]]; then
  echo "error: working tree not clean" >&2
  git status -sb
  exit 1
fi

if ! command -v gh >/dev/null 2>&1; then
  echo "error: gh CLI required" >&2
  exit 1
fi

DRY_RUN="${DRY_RUN:-}"
REPO="${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}"
DEFAULT_BRANCH="${DEFAULT_BRANCH:-main}"
DATE_UTC="$(date -u +%Y-%m-%d)"
SYNC_BRANCH="sync/upstream-${DATE_UTC}"

git config user.name "${GIT_AUTHOR_NAME:-github-actions[bot]}"
git config user.email "${GIT_AUTHOR_EMAIL:-41898282+github-actions[bot]@users.noreply.github.com}"

if [[ -z "${UPSTREAM:-}" ]]; then
  UPSTREAM="$(gh api "repos/${REPO}" --jq '.parent.full_name // empty')"
fi
if [[ -z "$UPSTREAM" ]]; then
  echo "error: ${REPO} has no fork parent; set UPSTREAM=owner/name" >&2
  exit 1
fi

UPSTREAM_OWNER="${UPSTREAM%%/*}"
echo "==> fork=${REPO}  upstream=${UPSTREAM}  branch=${DEFAULT_BRANCH}"

git remote remove upstream 2>/dev/null || true
git remote add upstream "https://github.com/${UPSTREAM}.git"
git fetch origin "$DEFAULT_BRANCH"
git fetch upstream "$DEFAULT_BRANCH"

git checkout -B "$DEFAULT_BRANCH" "origin/${DEFAULT_BRANCH}"

if git merge-base --is-ancestor "upstream/${DEFAULT_BRANCH}" HEAD; then
  echo "==> already contains upstream/${DEFAULT_BRANCH}; nothing to merge"
  exit 0
fi

echo "==> commits to merge:"
git log --oneline "HEAD..upstream/${DEFAULT_BRANCH}" | head -40

if [[ -n "$DRY_RUN" ]]; then
  echo "==> DRY_RUN: stop before merge"
  exit 0
fi

pr_body() {
  local extra="$1"
  cat <<EOF
Automatic sync of [\`${UPSTREAM}\`](https://github.com/${UPSTREAM}) into this fork did not update \`${DEFAULT_BRANCH}\` on its own.

${extra}

Do **not** reset this fork to upstream (fork-only commits would be dropped). Prefer a merge commit.

This is not a release. Release still requires an annotated \`vX.Y.Z\` tag, so \`release.yml\` will not run from this sync.
EOF
}

existing_open_pr() {
  local head="$1"
  gh pr list --repo "$REPO" --state open --base "$DEFAULT_BRANCH" --head "$head" \
    --json url --jq '.[0].url // empty' 2>/dev/null || true
}

comment_or_create_pr() {
  local head="$1"
  local title="$2"
  local body="$3"
  local url
  url="$(existing_open_pr "$head")"
  if [[ -n "$url" ]]; then
    echo "==> already open: $url"
    gh pr comment "$url" --repo "$REPO" --body "$body" || true
    return 0
  fi
  if ! url="$(
    gh pr create --repo "$REPO" --base "$DEFAULT_BRANCH" --head "$head" \
      --title "$title" --body "$body"
  )"; then
    return 1
  fi
  echo "==> opened PR: $url"
}

open_issue() {
  local body="$1"
  gh issue create --repo "$REPO" \
    --title "Upstream sync needs a human (${DATE_UTC})" \
    --body "$body"
}

TITLE="Sync upstream ${UPSTREAM} (${DATE_UTC})"

if git merge --no-edit --no-ff "upstream/${DEFAULT_BRANCH}"; then
  echo "==> merge clean"
  if git push origin "$DEFAULT_BRANCH"; then
    echo "==> pushed ${DEFAULT_BRANCH} (no tag → release.yml will not run)"
    exit 0
  fi
  echo "==> push to ${DEFAULT_BRANCH} failed (often GITHUB_TOKEN + workflow YAML)"
  git checkout -B "$SYNC_BRANCH"
  git push -u origin "$SYNC_BRANCH"
  BODY="$(pr_body "$(cat <<EOF
The merge itself was **clean**, but pushing \`${DEFAULT_BRANCH}\` was rejected.

Usual cause: default \`GITHUB_TOKEN\` cannot update \`.github/workflows/*\`.
Add repo secret \`SYNC_UPSTREAM_TOKEN\` (PAT: Contents + Workflows + Pull requests; classic \`repo\` + \`workflow\`) and re-run **Actions → sync-upstream**.

Branch \`${SYNC_BRANCH}\` already contains the merge commit — merging this PR is enough.
EOF
)")"
  if comment_or_create_pr "$SYNC_BRANCH" "$TITLE" "$BODY"; then
    exit 0
  fi
  open_issue "$BODY"
  exit 0
fi

echo "==> merge conflict; aborting (no force-push)"
CONFLICTS="$(git diff --name-only --diff-filter=U || true)"
git merge --abort
git checkout -B "$DEFAULT_BRANCH" "origin/${DEFAULT_BRANCH}"
BODY="$(pr_body "$(cat <<EOF
**Merge conflicts** — nothing was force-pushed.

Conflicted files:
\`\`\`
${CONFLICTS:-"(git did not list paths; use the GitHub conflict UI)"}
\`\`\`
EOF
)")"

if comment_or_create_pr "${UPSTREAM_OWNER}:${DEFAULT_BRANCH}" "$TITLE" "$BODY"; then
  exit 0
fi

git checkout -B "$SYNC_BRANCH" "origin/${DEFAULT_BRANCH}"
git push -u origin "$SYNC_BRANCH"
BODY_BRANCH="$(pr_body "$(cat <<EOF
**Merge conflicts** — nothing was force-pushed. Cross-fork PR create failed, so this branch matches fork \`${DEFAULT_BRANCH}\` at sync time.

Conflicted files:
\`\`\`
${CONFLICTS:-"(git did not list paths)"}
\`\`\`

\`\`\`bash
git fetch https://github.com/${UPSTREAM}.git ${DEFAULT_BRANCH}:upstream-main
git checkout ${SYNC_BRANCH}
git merge --no-ff --no-edit upstream-main
# resolve, push, merge this PR
\`\`\`
EOF
)")"
if comment_or_create_pr "$SYNC_BRANCH" "$TITLE" "$BODY_BRANCH"; then
  exit 0
fi

open_issue "$BODY"
exit 0
