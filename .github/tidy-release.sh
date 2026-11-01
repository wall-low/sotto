#!/bin/bash
# Даёт файлам релиза понятные имена, убирает лишние и ставит описание.
#   .github/tidy-release.sh v1.0.0
set -euo pipefail

TAG="$1"
REPO="${GITHUB_REPOSITORY:-wall-low/sotto}"
NOTES="$(dirname "$0")/release-notes.md"

# Черновик не находится по тегу, поэтому ищем в общем списке
release_id=$(gh api "repos/$REPO/releases" --jq ".[] | select(.tag_name == \"$TAG\") | .id" | head -n 1)
if [ -z "$release_id" ]; then
  echo "Релиз $TAG не найден" >&2
  exit 1
fi

gh api "repos/$REPO/releases/$release_id/assets" --paginate --jq '.[] | "\(.id) \(.name)"' |
  while read -r id name; do
    case "$name" in
      *_aarch64.dmg) new="Sotto-macOS-Apple-Silicon.dmg" ;;
      *_x64.dmg) new="Sotto-macOS-Intel.dmg" ;;
      *-setup.exe) new="Sotto-Windows-Setup.exe" ;;
      *_amd64.AppImage) new="Sotto-Linux-x64.AppImage" ;;
      *_aarch64.AppImage) new="Sotto-Linux-ARM64.AppImage" ;;
      *_amd64.deb) new="Sotto-Linux-x64.deb" ;;
      *_arm64.deb) new="Sotto-Linux-ARM64.deb" ;;
      *.x86_64.rpm) new="Sotto-Linux-x64.rpm" ;;
      *.aarch64.rpm) new="Sotto-Linux-ARM64.rpm" ;;
      *.msi | *.app.tar.gz)
        gh api -X DELETE "repos/$REPO/releases/assets/$id"
        echo "удалён $name"
        continue
        ;;
      *) continue ;;
    esac
    gh api -X PATCH "repos/$REPO/releases/assets/$id" -f name="$new" >/dev/null
    echo "$name -> $new"
  done

# Тег передаётся явно: иначе у черновика он может слететь на untagged-...
gh api -X PATCH "repos/$REPO/releases/$release_id" -f tag_name="$TAG" -F body=@"$NOTES" >/dev/null
