#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="${RIVET_PAPER_RUNTIME_JAR:-$root/work/run/versions/26.2/paper-26.2.jar}"
libraries="${RIVET_PAPER_LIBRARIES:-$root/work/run/libraries}"
java_home="${RIVET_JAVA_HOME:-${JAVA_HOME:-}}"
java_cmd="${java_home:+$java_home/bin/}java"
javac_cmd="${java_home:+$java_home/bin/}javac"
commit="$(unzip -p "$runtime" META-INF/MANIFEST.MF | tr -d '\r' | sed -n 's/^Git-Commit: //p')"
if [[ "$commit" != "0a99345" ]]; then
    echo "Expected pinned Paper commit 0a99345; got $commit" >&2
    exit 1
fi
classes="$root/.cache/block-pos-set-java"
mkdir -p "$classes"
classpath="$runtime"
while IFS= read -r -d '' library; do classpath="$classpath:$library"; done < <(find "$libraries" -name '*.jar' -print0)
"$javac_cmd" -cp "$classpath" -d "$classes" "$root/src/java/BlockPosSetProbe.java"
"$java_cmd" --enable-native-access=ALL-UNNAMED --add-opens java.base/java.util=ALL-UNNAMED -cp "$classes:$classpath" BlockPosSetProbe |
    sed -n 's/^BLOCK_POS_SET\t//p'
