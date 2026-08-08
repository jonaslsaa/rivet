#!/usr/bin/env bash
# Compile and run the Java ground-truth surrogate probe (GitHub #264).
# Prints JSON Lines on stdout; diagnostics on stderr.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE"

if [[ -n "${RIVET_JAVA_HOME:-}" ]]; then
  JAVAC="$RIVET_JAVA_HOME/bin/javac"; JAVA="$RIVET_JAVA_HOME/bin/java"
elif [[ -d "${JAVA_HOME:-}" ]]; then
  JAVAC="$JAVA_HOME/bin/javac"; JAVA="$JAVA_HOME/bin/java"
else
  JAVAC="$(command -v javac)"; JAVA="$(command -v java)"
fi

GRADLE_CACHE="${HOME}/.gradle/caches/modules-2/files-2.1"
find_jar() { find "$GRADLE_CACHE/$1/$2" -name "$2-$3*.jar" 2>/dev/null | grep -v sources | head -1 || true; }

NETTY_BUF="${NETTY_BUF:-$(find_jar io.netty netty-buffer 4.2.15.Final)}"
NETTY_COMMON="${NETTY_COMMON:-$(find_jar io.netty netty-common 4.2.15.Final)}"
GSON="${GSON:-$(find "$HOME/.gradle/wrapper/dists" -name 'gson-2.13.1.jar' 2>/dev/null | head -1)}"

if [[ -z "$NETTY_BUF" || -z "$NETTY_COMMON" ]]; then
  echo "error: netty 4.2.15 jars not found (set NETTY_BUF/NETTY_COMMON)" >&2
  exit 64
fi
if [[ -z "$GSON" ]]; then
  echo "error: gson jar not found (set GSON)" >&2
  exit 64
fi

CP="$NETTY_BUF:$NETTY_COMMON:$GSON"
mkdir -p out
"$JAVAC" -cp "$CP" -d out java/SurrogateProbe.java
"$JAVA" -cp "out:$CP" SurrogateProbe
