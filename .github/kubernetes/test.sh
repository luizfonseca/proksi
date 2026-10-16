#!/usr/bin/env bash
# Runs proksi as an ingress controller in the current cluster (kind, in CI) and checks that
# Ingresses are served, follow their pods, and are removed with them.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"

on_failure() {
  echo "--- proksi logs"
  kubectl -n proksi logs deployment/proksi --tail=200 || true
  echo "--- objects"
  kubectl get ingress,svc,endpointslices,pods -A || true
}
trap on_failure ERR

kubectl apply -f "$here/../../deploy/kubernetes/proksi.yaml"
kubectl -n proksi set image deployment/proksi proksi=proksi:ci
kubectl -n proksi rollout status deployment/proksi --timeout=120s

kubectl apply -f "$here/web.yaml"
kubectl rollout status deployment/web --timeout=120s

kubectl -n proksi port-forward service/proksi 8080:80 >/dev/null 2>&1 &
trap 'kill %1 2>/dev/null || true' EXIT

# Retries `curl` for up to 60 seconds until `check` passes on its output
expect() {
  local description="$1" check="$2" path="${3:-/}" body=""
  for _ in $(seq 1 60); do
    body="$(curl -s -o - -w ' %{http_code}' -H 'Host: web.test' "http://127.0.0.1:8080$path" || true)"
    if eval "$check"; then
      echo "ok: $description"
      return 0
    fi
    sleep 1
  done
  echo "failed: $description (last response: $body)"
  return 1
}

expect "the Ingress is served" '[[ "$body" == *"hello from web"*" 200" ]]'
expect "paths below the prefix too" '[[ "$body" == *"hello from web"*" 200" ]]' "/any/path"

kubectl scale deployment/web --replicas=0
expect "no ready pods: 503" '[[ "$body" == *" 503" ]]'

kubectl scale deployment/web --replicas=2
expect "pods back" '[[ "$body" == *"hello from web"*" 200" ]]'

kubectl delete ingress web
expect "the Ingress removed: an unknown host" '[[ "$body" != *"hello from web"* ]]'
