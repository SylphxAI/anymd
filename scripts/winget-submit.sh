#!/usr/bin/env bash
# Render the Sylphx.anymd winget manifests from the current GitHub release and
# open one pull request on microsoft/winget-pkgs. A release that does not exist
# yet, or a version already proposed, is a no-op. WINGET_TOKEN is optional: the
# manifests are still rendered and checked, and the pull request is skipped.
#
# The token has to be able to push a branch and open a pull request on
# microsoft/winget-pkgs. A fine-grained PAT scoped to that repository does; the
# anymd GITHUB_TOKEN does not, because it can only write to anymd.
set -euo pipefail

name=anymd
publisher=Sylphx
repo="${GITHUB_REPOSITORY:-SylphxAI/anymd}"
v="$(jq -r .version packages/anymd/package.json)"
id="${publisher}.${name}"
winget_repo="microsoft/winget-pkgs"
manifest_dir="manifests/s/Sylphx/anymd/${v}"

if ! gh release view "v${v}" --repo "$repo" >/dev/null 2>&1; then
  echo "no release v${v} yet; nothing to publish to winget"
  exit 0
fi

mkdir -p dl
gh release download "v${v}" --repo "$repo" --dir dl --pattern "${name}-win32-x64-msvc.zip"
(cd dl && sha256sum -- * > ../SHA256SUMS)

node scripts/winget-manifest.mjs \
  --name "$name" \
  --version "$v" \
  --repo "$repo" \
  --publisher "$publisher" \
  --sums SHA256SUMS \
  --package-json packages/anymd/package.json \
  --out out

test -s "out/${id}.yaml"
test -s "out/${id}.installer.yaml"
test -s "out/${id}.locale.en-US.yaml"
grep -q 'RelativeFilePath: anymd.exe' "out/${id}.installer.yaml"
grep -q "PackageVersion: ${v}" "out/${id}.yaml"

# Rendering stops here unless this is the submit step. The pull request needs a
# token that can push to a fork of microsoft/winget-pkgs, and that token is
# GH_TOKEN for that step alone. The anymd release was already read above with
# the job's own token, so the two credentials never cross.
if [ "${1:-}" != "submit" ]; then
  echo "rendered and checked the Sylphx.anymd manifest for ${v}"
  exit 0
fi

if [ ! -s "out/${id}.yaml" ]; then
  echo "no rendered manifest for ${v}; nothing to submit"
  exit 0
fi

# The submit job passes its credential as GH_TOKEN. When that secret is absent
# the variable is empty and there is nothing to submit with; the release stands.
if [ -z "${GH_TOKEN:-}" ]; then
  echo "::notice::no winget credential set; the manifest for ${v} was checked and the pull request was skipped"
  exit 0
fi

existing="$(gh pr list --repo "$winget_repo" --state all --search "Sylphx.anymd ${v} in:title" --json number,state,title --jq '.[0].number // empty')"
if [ -n "$existing" ]; then
  echo "winget already has a pull request for ${id} ${v} (#${existing}); leaving it"
  exit 0
fi

work="$(mktemp -d)"
gh repo fork "$winget_repo" --clone=false >/dev/null 2>&1 || true
gh repo clone "$winget_repo" "$work/winget-pkgs" -- --depth 1
git -C "$work/winget-pkgs" checkout -b "anymd-${v}"
mkdir -p "$work/winget-pkgs/${manifest_dir}"
cp "out/${id}.yaml" "out/${id}.installer.yaml" "out/${id}.locale.en-US.yaml" "$work/winget-pkgs/${manifest_dir}/"
git -C "$work/winget-pkgs" add "$manifest_dir"
if git -C "$work/winget-pkgs" diff --cached --quiet; then
  echo "winget-pkgs already carries ${id} ${v}"
  exit 0
fi
git -C "$work/winget-pkgs" \
  -c user.name="github-actions[bot]" \
  -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
  commit -q -m "New package: ${id} version ${v}"

owner="${WINGET_FORK_OWNER:-}"
if [ -z "$owner" ]; then
  owner="$(gh api user --jq .login)"
fi
# gh authenticates the push from GH_TOKEN, so the token is never written into a
# remote URL or a command argument. The clone's origin is microsoft/winget-pkgs,
# which this token cannot push to; the fork can.
gh -R "${owner}/winget-pkgs" auth setup-git
git -C "$work/winget-pkgs" remote add fork "https://github.com/${owner}/winget-pkgs.git"
git -C "$work/winget-pkgs" push -q fork "HEAD:anymd-${v}"
gh pr create --repo "$winget_repo" --head "${owner}:anymd-${v}" --base master \
  --title "New package: ${id} version ${v}" \
  --body "Adds the portable Windows build of anymd ${v} from the SylphxAI/anymd release. The archive contains anymd.exe at its root."
echo "opened the winget pull request for ${id} ${v}"
