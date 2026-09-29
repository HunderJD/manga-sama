#!/usr/bin/env bash
# Checks that every i18n key used in src/ (t("key"), tf("key", ...), and the
# "help.xxx" literals in HELP tuples) exists as "key": in every locales/*.json.
# Run: scripts/check-i18n.sh (from anywhere)
cd "$(dirname "${BASH_SOURCE[0]}")/.."

src=$(find src -name '*.rs' -print0 | xargs -0 cat)
keys=$( { echo "$src" | tr '\n' ' ' | grep -oP '\bt(f)?\(\s*"\K[^"]+'
          echo "$src" | grep -oP '"\Khelp\.[A-Za-z0-9_]+'; } | sort -u )

missing=0
for file in locales/*.json; do
  for key in $keys; do
    grep -qF "\"$key\":" "$file" || { echo "missing: $key ($file)"; missing=1; }
  done
done

[ "$missing" -eq 0 ] && echo "all keys present" || exit 1
