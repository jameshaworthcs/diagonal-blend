#!/bin/sh
# Lighthouse against a URL (the deployed site by default), mobile and
# desktop. Reports go to lighthouse/ (ignored by git); scores are printed.
set -eu
URL=${1:-https://blend.cloud.jhaworth.ai/}
OUT=lighthouse
mkdir -p "$OUT"
for preset in mobile desktop; do
  flag=""
  [ "$preset" = desktop ] && flag="--preset=desktop"
  npx -y lighthouse@13.5.0 "$URL" $flag --quiet --chrome-flags="--headless=new" \
    --only-categories=performance,accessibility,best-practices,seo \
    --output=json --output=html --output-path="$OUT/$preset"
  node -e '
    const r = require(process.argv[1]);
    const s = Object.values(r.categories).map((c) => `${c.title}: ${Math.round(c.score * 100)}`).join(", ");
    console.log(`${process.argv[2]}: ${s}`);
  ' "$PWD/$OUT/$preset.report.json" "$preset"
done
