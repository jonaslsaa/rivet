#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="${RIVET_PAPER_RUNTIME_JAR:-$root/work/run/versions/26.2/paper-26.2.jar}"
libraries="${RIVET_PAPER_LIBRARIES:-$root/work/run/libraries}"
java_home="${RIVET_JAVA_HOME:-${JAVA_HOME:-}}"
commit="$(unzip -p "$runtime" META-INF/MANIFEST.MF | tr -d '\r' | sed -n 's/^Git-Commit: //p')"
[[ "$commit" == "0a99345" ]] || { echo "Expected Paper 0a99345, got $commit" >&2; exit 1; }
classes="$root/.cache/directory-lock-java"
mkdir -p "$classes"
classpath="$runtime"
while IFS= read -r -d '' library; do classpath="$classpath:$library"; done < <(find "$libraries" -name '*.jar' -print0)
"${java_home:+$java_home/bin/}javac" -cp "$classpath" -d "$classes" "$root/src/java/DirectoryLockProbe.java"
cd "$root/../.."
RIVET_LOCK_JAVA="${java_home:+$java_home/bin/}java" \
RIVET_LOCK_CLASSPATH="$classes:$classpath" \
CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 \
cargo test -p rivet-server --lib directory_lock -- --include-ignored
