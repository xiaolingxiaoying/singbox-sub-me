#!/usr/bin/env sh
# Runs the release-artifact acceptance suite in the two supported distributions.
# SBCTL_ARTIFACT must name the Linux release binary being accepted.
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
artifact=${SBCTL_ARTIFACT:?set SBCTL_ARTIFACT to the Linux release binary to accept}
test -f "$artifact" || { echo "SBCTL_ARTIFACT is not a file: $artifact" >&2; exit 2; }

for image in debian:12-slim ubuntu:24.04; do
  tag="sbctl-acceptance-$(printf '%s' "$image" | tr ':/' '--')"
  docker build --build-arg "BASE_IMAGE=$image" -f "$repository_root/tests/acceptance/Dockerfile" -t "$tag" "$repository_root"
  docker run --rm -v "$artifact:/opt/sbctl/sbctl:ro" -e SBCTL_BIN=/opt/sbctl/sbctl "$tag"
done
