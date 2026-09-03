#!/usr/bin/env sh
# Runs the release-artifact acceptance suites in the two supported distributions.
# SBCTL_ARTIFACT must name the Linux release binary being accepted.
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
artifact=${SBCTL_ARTIFACT:?set SBCTL_ARTIFACT to the Linux release binary to accept}
test -f "$artifact" || { echo "SBCTL_ARTIFACT is not a file: $artifact" >&2; exit 2; }
artifact=$(CDPATH= cd -- "$(dirname "$artifact")" && pwd)/$(basename "$artifact")

for image in debian:12-slim ubuntu:22.04 ubuntu:24.04; do
  tag="sbctl-acceptance-$(printf '%s' "$image" | tr ':/' '--')"
  docker build --build-arg "BASE_IMAGE=$image" -f "$repository_root/tests/acceptance/Dockerfile" -t "$tag" "$repository_root"
  container="${tag}-$(date +%s)-$$"
  cleanup() { MSYS_NO_PATHCONV=1 docker rm -f "$container" >/dev/null 2>&1 || true; }
  trap cleanup EXIT INT TERM

  docker_artifact=$artifact
  if command -v cygpath >/dev/null 2>&1; then
    docker_artifact=$(cygpath -w "$artifact")
  fi
  MSYS_NO_PATHCONV=1 docker run -d --name "$container" --privileged --cgroupns=host \
    -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
    -v "$docker_artifact:/opt/sbctl/sbctl:ro" "$tag" >/dev/null

  ready=false
  for _ in $(seq 1 30); do
    state=$(docker exec "$container" systemctl is-system-running 2>/dev/null || true)
    if [ "$state" = running ] || [ "$state" = degraded ]; then
      ready=true
      break
    fi
    sleep 1
  done
  [ "$ready" = true ] || { docker logs "$container" >&2; exit 1; }

  MSYS_NO_PATHCONV=1 docker exec "$container" /usr/local/lib/sbctl-acceptance/verify-bootstrap.sh
  # Keep the release artifact outside sbctl's managed installation path: a
  # purge deliberately removes /usr/local/bin/sbctl, while this suite needs to
  # verify a subsequent fresh install in the same container.
  # The bootstrap verifier temporarily installs a stub at the managed path;
  # replace it so the generated systemd service starts the release artifact.
  MSYS_NO_PATHCONV=1 docker exec "$container" cp /opt/sbctl/sbctl /usr/local/bin/sbctl
  MSYS_NO_PATHCONV=1 docker exec "$container" env SBCTL_BIN=/opt/sbctl/sbctl \
    /usr/local/lib/sbctl-acceptance/verify.sh
  MSYS_NO_PATHCONV=1 docker exec "$container" env SBCTL_BIN=/opt/sbctl/sbctl sbctl-acceptance-real
  cleanup
  trap - EXIT INT TERM
done
