#!/usr/bin/env bash
# Boot one fresh Paper 26.2 server with the ArenaTraceProbe plugin and record
# the tick traces of the named arenas (tools/rivet-oracle/arenas/README.md).
#
#   arena_trace_paper.sh <out_dir> <arena>[,<arena>...]
#
# Writes <out_dir>/traces/<arena>.trace.jsonl and <out_dir>/server.log. The
# run dir <out_dir>/run is recreated from scratch on every call, so each call
# is one fresh JVM and one fresh world (twin runs = two calls). Exits nonzero
# unless the probe wrote its DONE marker.
#
# Inputs: RIVET_PAPER_RUNTIME_JAR / RIVET_PAPER_LIBRARIES (default: the
# materialized runtime under tools/rivet-oracle/work/run), RIVET_JAVA_HOME or
# JAVA_HOME (Temurin 25), RIVET_ARENA_PORT (default 25611).
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${1:?usage: arena_trace_paper.sh <out_dir> <arena,...>}"
names="${2:?usage: arena_trace_paper.sh <out_dir> <arena,...>}"
runtime="${RIVET_PAPER_RUNTIME_JAR:-$root/work/run/versions/26.2/paper-26.2.jar}"
libraries="${RIVET_PAPER_LIBRARIES:-$root/work/run/libraries}"
java_home="${RIVET_JAVA_HOME:-${JAVA_HOME:-}}"
port="${RIVET_ARENA_PORT:-25611}"
timeout_s="${RIVET_ARENA_TIMEOUT:-600}"
java="${java_home:+$java_home/bin/}java"
javac="${java_home:+$java_home/bin/}javac"
jar="${java_home:+$java_home/bin/}jar"

[[ -f "$runtime" ]] || { echo "UNVERIFIED: Paper runtime jar missing: $runtime" >&2; exit 2; }
[[ -d "$libraries" ]] || { echo "UNVERIFIED: Paper libraries missing: $libraries" >&2; exit 2; }
commit="$(unzip -p "$runtime" META-INF/MANIFEST.MF | tr -d '\r' | sed -n 's/^Git-Commit: //p')"
[[ "$commit" == "0a99345" ]] || { echo "Expected Paper 0a99345, got $commit" >&2; exit 1; }

classpath="$runtime"
while IFS= read -r -d '' library; do classpath="$classpath:$library"; done < <(find "$libraries" -name '*.jar' -print0 | sort -z)

rm -rf "$out"
mkdir -p "$out/classes" "$out/run/plugins" "$out/run/config" "$out/traces"
out="$(cd "$out" && pwd)"
"$javac" -nowarn -cp "$classpath" -d "$out/classes" "$root/src/java/arena/ArenaTraceProbe.java" 2> "$out/javac.log" \
    || { cat "$out/javac.log" >&2; exit 1; }
cp "$root/src/java/arena/plugin.yml" "$out/classes/plugin.yml"
"$jar" cf "$out/run/plugins/RivetArenaTraceProbe.jar" -C "$out/classes" .

sed "s/^server-port=.*/server-port=$port/" "$root/arenas/server.properties" > "$out/run/server.properties"
echo "eula=true" > "$out/run/eula.txt"
cp "$root/fixtures/paper-global.yml" "$out/run/config/paper-global.yml"
cp "$root/fixtures/paper-world-defaults.yml" "$out/run/config/paper-world-defaults.yml"

(
    cd "$out/run"
    exec timeout --kill-after=20 "$timeout_s" "$java" -Xms512M -Xmx2G --enable-native-access=ALL-UNNAMED \
        -Drivet.arena.dir="$root/arenas" -Drivet.arena.out="$out/traces" -Drivet.arena.names="$names" \
        -cp "$classpath" org.bukkit.craftbukkit.Main nogui < /dev/null > "$out/server.log" 2>&1
) || { echo "Paper exited nonzero; see $out/server.log" >&2; }

if [[ -f "$out/traces/FAILED" ]]; then
    echo "arena probe FAILED: $(cat "$out/traces/FAILED")" >&2
    exit 1
fi
[[ -f "$out/traces/DONE" ]] || { echo "arena probe did not finish; see $out/server.log" >&2; exit 1; }
grep -q 'RIVET_ARENA_FROZEN' "$out/server.log" || { echo "frozen-simulation marker missing" >&2; exit 1; }
echo "recorded $(ls "$out/traces" | grep -c '\.trace\.jsonl$') arena trace(s) in $out/traces"
