#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$tool_dir/../.." && pwd)"
paper_libraries="${RIVET_PAPER_LIBRARIES:-$repo_dir/tools/rivet-oracle/work/run/libraries}"
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

classpath="$paper_jar"
while IFS= read -r -d '' library; do
    classpath="$classpath:$library"
done < <(find "$paper_libraries" -type f -name '*.jar' -print0)

class_file="$classes_dir/dev/rivet/oracle/RivetReferenceOracle.class"
if [[ ! -f "$class_file" || "$source_file" -nt "$class_file" ]]; then
    mkdir -p "$classes_dir"
    echo "Compiling Rivet reference oracle against $(basename "$paper_jar")" >&2
    "$javac_cmd" --release 25 -cp "$classpath" -d "$classes_dir" "$source_file"
fi

cd "$tool_dir/target"
exec "$java_cmd" --enable-native-access=ALL-UNNAMED \
    -cp "$classes_dir:$classpath" \
    dev.rivet.oracle.RivetReferenceOracle "$@"
