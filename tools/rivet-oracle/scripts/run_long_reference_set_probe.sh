#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="${RIVET_PAPER_RUNTIME_JAR:-$root/work/run/versions/26.2/paper-26.2.jar}"
libraries="${RIVET_PAPER_LIBRARIES:-$root/work/run/libraries}"
java_home="${RIVET_JAVA_HOME:-${JAVA_HOME:-}}"
commit="$(unzip -p "$runtime" META-INF/MANIFEST.MF | tr -d '\r' | sed -n 's/^Git-Commit: //p')"
[[ "$commit" == "0a99345" ]] || { echo "Expected Paper 0a99345, got $commit" >&2; exit 1; }
fastutil="$libraries/it/unimi/dsi/fastutil/8.5.18/fastutil-8.5.18.jar"
classes="$root/.cache/long-reference-set-java"
mkdir -p "$classes"
"${java_home:+$java_home/bin/}javac" -cp "$fastutil" -d "$classes" "$root/src/java/LongReferenceSetProbe.java"
"${java_home:+$java_home/bin/}java" -cp "$classes:$fastutil" LongReferenceSetProbe
