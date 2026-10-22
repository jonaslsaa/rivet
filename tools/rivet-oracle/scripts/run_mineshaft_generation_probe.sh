#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="${RIVET_PAPER_RUNTIME_JAR:-$root/work/run/versions/26.2/paper-26.2.jar}"
libraries="${RIVET_PAPER_LIBRARIES:-$root/work/run/libraries}"
java_home="${RIVET_JAVA_HOME:-${JAVA_HOME:-}}"
commit="$(unzip -p "$runtime" META-INF/MANIFEST.MF | tr -d '\r' | sed -n 's/^Git-Commit: //p')"
[[ "$commit" == "0a99345" ]] || { echo "Expected Paper 0a99345, got $commit" >&2; exit 1; }
classes="$root/.cache/structure-registry-java"
mkdir -p "$classes"
classpath="$runtime"
while IFS= read -r -d '' library; do classpath="$classpath:$library"; done < <(find "$libraries" -name '*.jar' -print0)
"${java_home:+$java_home/bin/}javac" -cp "$classpath" -d "$classes" "$root/src/java/MineshaftGenerationProbe.java"
probe_output="${1:-$(mktemp -t rivet-mineshaft-generation)}"
if [[ $# == 0 ]]; then trap 'rm -f "$probe_output"' EXIT; fi
perl -e 'alarm 180; exec @ARGV' "${java_home:+$java_home/bin/}java" -Xmx2G --enable-native-access=ALL-UNNAMED -cp "$classes:$classpath" MineshaftGenerationProbe "$probe_output" "${@:2}" >&2
if [[ $# == 0 ]]; then sed -n '/^{/p' "$probe_output"; fi
