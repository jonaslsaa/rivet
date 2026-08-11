# Releasing Rivet

Rivet releases use tags in this exact form:

```text
rivet-v<rivet-version>-mc<minecraft-version>
```

For the committed metadata, the first stable tag will be
`rivet-v0.1.0-mc26.2`. Prereleases use only the recognized SemVer identifiers
`alpha`, `beta`, or `rc`, for example `rivet-v0.1.0-rc.1-mc26.2`. The release
workflow marks those tags as GitHub prereleases automatically.

## Mandatory pre-tag procedure

A release tag is a trusted maintainer declaration that the local Paper oracle
gate passed. GitHub Actions validates the committed metadata and builds the
artifacts; it does not check out or run `working/Paper`, and it does not try to
re-prove the local oracle result.

Run these commands from a clean checkout of the commit to release. Use a Temurin
Java 25 JDK for both `java` and `javac`; the gate also needs Python 3 and at
least 160 MB free on the checkout filesystem:

```bash
cd /path/to/Rivet
export PATH="$HOME/.cargo/bin:$PATH"
git fetch origin main --tags
git switch main
git pull --ff-only origin main

# Build or refresh the local Paper oracle artifact when needed.
cd working/Paper
./gradlew :paper-server:build
cd ../..
mkdir -p tools/rivet-oracle/work/jars
cp working/Paper/paper-server/build/libs/paper-paperclip-26.2.local-SNAPSHOT.jar \
  tools/rivet-oracle/work/jars/

# Build the offline Azalea client required by the gate's Paper-vs-Rivet rows.
(cd tools/rivet-client && cargo build --locked)

# Materialize the Paper runtime and libraries required by --require-oracle.
# This is a local scratch tree under tools/rivet-oracle/work, not working/Paper.
rm -rf tools/rivet-oracle/work/run
mkdir -p tools/rivet-oracle/work/run/config
printf 'eula=true\n' > tools/rivet-oracle/work/run/eula.txt
cp tools/rivet-oracle/fixtures/server.properties tools/rivet-oracle/work/run/
cp tools/rivet-oracle/fixtures/paper-global.yml tools/rivet-oracle/work/run/config/
cp tools/rivet-oracle/fixtures/paper-world-defaults.yml tools/rivet-oracle/work/run/config/
(
  cd tools/rivet-oracle/work/run
  java -Xms512M -Xmx2G -jar ../jars/paper-paperclip-26.2.local-SNAPSHOT.jar nogui \
    </dev/null > paper-first-boot.log 2>&1 &
  paper_pid=$!
  while ! grep -q 'Done (' paper-first-boot.log; do
    if ! kill -0 "$paper_pid" 2>/dev/null; then
      wait "$paper_pid"
      exit 1
    fi
    sleep 2
  done
  kill -TERM "$paper_pid"
  wait "$paper_pid" || test "$?" -eq 143
)

# Check the exact release identity, then run the mandatory full local gate.
python3 scripts/validate_release_tag.py rivet-v0.1.0-mc26.2
./scripts/gate.sh --require-oracle

git status --short
git diff --exit-code

git tag -a rivet-v0.1.0-mc26.2 -m "Release Rivet 0.1.0 for Minecraft 26.2"
git push origin rivet-v0.1.0-mc26.2
```

For an alpha, beta, or release candidate, replace the tag and message in both
commands, for example:

```bash
python3 scripts/validate_release_tag.py rivet-v0.1.0-rc.1-mc26.2
./scripts/gate.sh --require-oracle
git tag -a rivet-v0.1.0-rc.1-mc26.2 -m "Release Rivet 0.1.0-rc.1 for Minecraft 26.2"
git push origin rivet-v0.1.0-rc.1-mc26.2
```

Do not commit anything under `working/`. It is local Paper source and scratch
state only. The `git diff --exit-code` check above is against the Rivet
checkout and should be run before creating the tag.

## What the tag workflow does

A push of a matching tag runs `.github/workflows/release.yml`:

1. It validates the tag against `[workspace.metadata.rivet]` in `Cargo.toml` and
   checks that the development package version remains consistent with that
   metadata.
2. It builds `rivet-server` natively for Linux x86_64, Linux ARM64, and macOS
   ARM64, and packages each binary with `README.md` and `LICENSE`.
3. It publishes a `SHA256SUMS` file covering all release archives.
4. It publishes two GHCR tags: the full release tag (for example
   `ghcr.io/jonaslsaa/rivet:rivet-v0.1.0-mc26.2`) and the moving Minecraft-line
   tag (`ghcr.io/jonaslsaa/rivet:mc26.2`). Before pushing, it queries GHCR and
   fails closed if the full release tag already exists or its absence cannot be
   proved; only the Minecraft-line tag is allowed to move.
5. It creates a draft GitHub Release. A maintainer reviews the archives and
   checksums and publishes the draft manually.

The workflow never publishes or updates `latest`. A stable release may receive
a `latest` tag later through a separate deliberate publication procedure after
the draft has been reviewed and published.

To test the release scripts without making a tag or release:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
python3 scripts/test_release_automation.py
```
