#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="$root/work/run/versions/26.2/paper-26.2.jar"
java_home="${RIVET_JAVA_HOME:-${JAVA_HOME:-}}"
commit="$(unzip -p "$runtime" META-INF/MANIFEST.MF | tr -d '\r' | sed -n 's/^Git-Commit: //p')"
[[ "$commit" == "0a99345" ]] || { echo "Unexpected Paper revision: $commit" >&2; exit 1; }
classes="$root/.cache/live-block-packets-java"
mkdir -p "$classes"
classpath="$runtime"
while IFS= read -r -d '' library; do classpath="$classpath:$library"; done < <(find "$root/work/run/libraries" -name '*.jar' -print0)
"${java_home:+$java_home/bin/}javac" -cp "$classpath" -d "$classes" "$root/src/java/LiveBlockPacketsProbe.java"
"${java_home:+$java_home/bin/}java" --enable-native-access=ALL-UNNAMED -cp "$classes:$classpath" LiveBlockPacketsProbe "$1"
