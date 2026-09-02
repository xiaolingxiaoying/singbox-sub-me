#!/usr/bin/env sh
# Runs the release-artifact acceptance suite in the two supported distributions.
# Set SBCTL_ARTIFACT to test a prebuilt Linux release binary instead of the image build.
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
artifact=${SBCTL_ARTIFACT:-}

for image in debian:12-slim ubuntu:24.04; do
  tag="sbctl-acceptance-$(printf '%s' "$image" | tr ':/' '--')"
  docker build --build-arg "BASE_IMAGE=$image" -f "$repository_root/tests/acceptance/Dockerfile" -t "$tag" "$repository_root"
  if [ -n "$artifact" ]; then
    docker run --rm -v "$artifact:/opt/sbctl/sbctl:ro" -e SBCTL_BIN=/opt/sbctl/sbctl "$tag"
  else
    docker run --rm "$tag"
  fi
done
