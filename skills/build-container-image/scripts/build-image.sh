#!/usr/bin/env bash
#
# Build (and optionally push) the Chronos container image from the
# repository-root Dockerfile, tagging it as ${repo}:${tag}.
#
# Defaults: --oci docker, --repo ghcr.io/maple52046/chronos, tag ${version}-${ts}.
# Only the docker OCI builder is supported for now.

set -euo pipefail

OCI="docker"
REPO="ghcr.io/maple52046/chronos"
TAG=""
PUSH=0

usage() {
    cat <<'USAGE'
Usage: build-image.sh [--oci <tool>] [--repo <repo>] [--tag <tag>] [--push]

  --oci <tool>    OCI builder to use (default: docker; only docker is supported)
  --repo <repo>   Image repository (default: ghcr.io/maple52046/chronos)
  --tag <tag>     Image tag (default: <version>-<ts>)
  --push          Push the image after a successful build
  -h, --help      Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --oci)
            OCI="${2:-}"
            shift 2
            ;;
        --repo)
            REPO="${2:-}"
            shift 2
            ;;
        --tag)
            TAG="${2:-}"
            shift 2
            ;;
        --push)
            PUSH=1
            shift
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "${OCI}" != "docker" ]]; then
    echo "error: --oci '${OCI}' is not supported; only 'docker' is supported for now" >&2
    exit 2
fi

# Resolve the repository root from this script's location so the build context
# and Cargo.toml lookup work regardless of the caller's working directory.
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../../.." >/dev/null 2>&1 && pwd)"

if [[ -z "${TAG}" ]]; then
    VERSION="$(grep -m1 '^version' "${REPO_ROOT}/Cargo.toml" | cut -d'"' -f2)"
    if [[ -z "${VERSION}" ]]; then
        echo "error: could not read workspace.package.version from Cargo.toml" >&2
        exit 1
    fi
    TS="$(date -u +%Y%m%d%H%M%S)"
    TAG="${VERSION}-${TS}"
fi

IMAGE="${REPO}:${TAG}"

echo "Building image: ${IMAGE}"
docker build -t "${IMAGE}" "${REPO_ROOT}"

if [[ "${PUSH}" -eq 1 ]]; then
    echo "Pushing image: ${IMAGE}"
    docker push "${IMAGE}"
    echo "Pushed: ${IMAGE}"
else
    echo "Built (not pushed): ${IMAGE}"
fi
