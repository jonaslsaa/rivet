#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$tool_dir/../.." && pwd)"
paper_libraries="${RIVET_PAPER_LIBRARIES:-$repo_dir/tools/rivet-oracle/work/run/libraries}"
paper_runtime_jar="${RIVET_PAPER_RUNTIME_JAR:-$(dirname "$paper_libraries")/versions/26.2/paper-26.2.jar}"
classes_dir="$tool_dir/target/classes"
source_file="$tool_dir/src/RivetReferenceOracle.java"

java_cmd=""
javac_cmd=""
java_homes=(
    "${RIVET_JAVA_HOME:-}"
    "${JAVA_HOME:-}"
    "${SDKMAN_CANDIDATES_DIR:-$HOME/.sdkman/candidates}/java/current"
)
if [[ "$(uname -s)" == "Darwin" && -x /usr/libexec/java_home ]]; then
    java_homes+=("$(/usr/libexec/java_home -v 25 2>/dev/null || true)")
fi

for java_home in "${java_homes[@]}"; do
    if [[ -x "$java_home/bin/javac" ]] \
        && "$java_home/bin/javac" -version 2>&1 | grep -Eq '^javac 25([. ]|$)'; then
        java_cmd="$java_home/bin/java"
        javac_cmd="$java_home/bin/javac"
        break
    fi
done

if [[ -z "$java_cmd" ]] && javac -version 2>&1 | grep -Eq '^javac 25([. ]|$)'; then
    java_cmd="$(command -v java)"
    javac_cmd="$(command -v javac)"
fi

if [[ -z "$java_cmd" ]]; then
    echo "Java 25 JDK not found; set RIVET_JAVA_HOME to its installation directory" >&2
    exit 1
fi

if [[ -n "${RIVET_PAPER_JAR:-}" ]]; then
    paper_jar="$RIVET_PAPER_JAR"
else
    paper_jar=""
    for candidate in "$repo_dir"/working/Paper/paper-server/build/libs/paper-server-*.jar; do
        if [[ -f "$candidate" ]]; then
            paper_jar="$candidate"
            break
        fi
    done
fi

if [[ -z "$paper_jar" || ! -f "$paper_jar" ]]; then
    echo "Paper server jar not found; build working/Paper first or set RIVET_PAPER_JAR" >&2
    exit 1
fi

if [[ ! -d "$paper_libraries" ]]; then
    echo "Paper runtime libraries not found at $paper_libraries" >&2
    echo "Boot the M0 Paper fixture server once or set RIVET_PAPER_LIBRARIES" >&2
    exit 1
fi

if [[ ! -f "$paper_runtime_jar" ]]; then
    echo "Materialized Paper runtime jar not found at $paper_runtime_jar" >&2
    echo "Boot Paper once or set RIVET_PAPER_RUNTIME_JAR" >&2
    exit 1
fi

paper_sha256="$(shasum -a 256 "$paper_jar" | awk '{print $1}')"
runtime_sha256="$(shasum -a 256 "$paper_runtime_jar" | awk '{print $1}')"
if [[ "$paper_sha256" != "$runtime_sha256" ]]; then
    echo "Paper compile jar and materialized runtime jar do not match" >&2
    echo "compile: $paper_sha256  $paper_jar" >&2
    echo "runtime: $runtime_sha256  $paper_runtime_jar" >&2
    exit 1
fi

manifest="$(unzip -p "$paper_jar" META-INF/MANIFEST.MF)"
paper_specification="$(printf '%s\n' "$manifest" | awk -F': ' '$1 == "Specification-Version" {gsub("\\r", "", $2); print $2; exit}')"
paper_implementation="$(printf '%s\n' "$manifest" | awk -F': ' '$1 == "Implementation-Version" {gsub("\\r", "", $2); print $2; exit}')"
paper_commit="$(printf '%s\n' "$manifest" | awk -F': ' '$1 == "Git-Commit" {gsub("\\r", "", $2); print $2; exit}')"
if [[ "$paper_specification" != 26.2* || -z "$paper_implementation" || -z "$paper_commit" ]]; then
    echo "Expected a Paper 26.2 server jar, got specification '$paper_specification'" >&2
    exit 1
fi

classpath="$paper_jar"
while IFS= read -r -d '' library; do
    classpath="$classpath:$library"
done < <(find "$paper_libraries" -type f -name '*.jar' -print0)

mkdir -p "$classes_dir"
echo "Compiling Rivet reference oracle against $paper_implementation ($paper_commit)" >&2
"$javac_cmd" --release 25 -cp "$classpath" -d "$classes_dir" "$source_file"

cd "$tool_dir/target"
exec "$java_cmd" --enable-native-access=ALL-UNNAMED \
    -Drivet.paper.sha256="$paper_sha256" \
    -Drivet.paper.specification="$paper_specification" \
    -Drivet.paper.implementation="$paper_implementation" \
    -Drivet.paper.commit="$paper_commit" \
    -cp "$classes_dir:$classpath" \
    dev.rivet.oracle.RivetReferenceOracle "$@"
