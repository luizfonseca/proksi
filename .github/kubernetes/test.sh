#!/usr/bin/env bash
# Runs proksi as an ingress controller in the current cluster (kind, in CI) and checks that
# Ingresses are served, follow their pods, and are removed with them.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"

on_failure() {
  echo "--- proksi logs"
  kubectl -n proksi logs deployment/proksi --tail=200 || true
  kubectl -n proksi describe pods || true
  echo "--- objects"
  kubectl get ingress,svc,endpointslices,pods -A || true
}
trap on_failure ERR

# The plain manifest is valid (checked by the API server, not applied)
kubectl create namespace proksi
kubectl apply --dry-run=server -f "$here/../../deploy/kubernetes/proksi.yaml" >/dev/null
echo "ok: the manifest is valid"

# Installed with the Helm chart. kind has no load balancers: with a ClusterIP service, its
# cluster IP is published instead
helm lint "$here/../../deploy/helm/proksi"
helm install proksi "$here/../../deploy/helm/proksi" --namespace proksi \
  --set image.repository=proksi --set image.tag=ci --set service.type=ClusterIP
kubectl -n proksi rollout status deployment/proksi --timeout=120s

# The certificate of secure.test, in a TLS secret
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=secure.test" \
  -keyout /tmp/secure.key -out /tmp/secure.crt 2>/dev/null
kubectl create secret tls web-tls --cert /tmp/secure.crt --key /tmp/secure.key

# The settings of basic_auth for auth.test
kubectl create secret generic web-auth --from-literal=user=admin --from-literal=pass=secret
kubectl label secret web-auth proksi.ingress/auth=true

kubectl apply -f "$here/web.yaml"
kubectl rollout status deployment/web --timeout=120s

kubectl -n proksi port-forward service/proksi 8080:80 8443:443 >/dev/null 2>&1 &
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

# HTTPS with the secret's certificate, through the service
for _ in $(seq 1 60); do
  subject="$(openssl s_client -connect 127.0.0.1:8443 -servername secure.test </dev/null 2>/dev/null \
    | openssl x509 -noout -subject 2>/dev/null || true)"
  [[ "$subject" == *"secure.test"* ]] && break
  sleep 1
done
[[ "$subject" == *"secure.test"* ]] || { echo "failed: certificate from the secret ($subject)"; false; }
echo "ok: the certificate from the TLS secret"
body="$(curl -sk --resolve secure.test:8443:127.0.0.1 https://secure.test:8443/)"
[[ "$body" == *"hello from web"* ]] || { echo "failed: HTTPS through the service ($body)"; false; }
echo "ok: HTTPS through the service"

# cert-manager's HTTP-01 solver: an Ingress for the challenge path of a host with TLS, with the
# class in the annotation it uses, answered over plain HTTP instead of redirected to HTTPS
kubectl apply -f - <<'YAML'
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: web-solver
  annotations:
    kubernetes.io/ingress.class: proksi
spec:
  rules:
    - host: secure.test
      http:
        paths:
          - path: /.well-known/acme-challenge/token-123
            pathType: ImplementationSpecific
            backend:
              service:
                name: web
                port:
                  number: 80
YAML
for _ in $(seq 1 60); do
  body="$(curl -s -w ' %{http_code}' -H 'Host: secure.test' \
    http://127.0.0.1:8080/.well-known/acme-challenge/token-123 || true)"
  [[ "$body" == *"hello from web"*" 200" ]] && break
  sleep 1
done
[[ "$body" == *"hello from web"*" 200" ]] || { echo "failed: the solver's challenge ($body)"; false; }
echo "ok: the HTTP-01 solver's challenge is proxied over HTTP"
status="$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: secure.test' http://127.0.0.1:8080/page)"
[[ "$status" == "308" ]] || { echo "failed: other paths of secure.test ($status)"; false; }
echo "ok: other paths of the host are still redirected to HTTPS"

# Plugins from the annotation
for _ in $(seq 1 60); do
  status="$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: auth.test' http://127.0.0.1:8080/ || true)"
  [[ "$status" == "401" ]] && break
  sleep 1
done
[[ "$status" == "401" ]] || { echo "failed: basic_auth without credentials ($status)"; false; }
echo "ok: basic_auth from the annotation"
response="$(curl -si -u admin:secret -H 'Host: auth.test' http://127.0.0.1:8080/)"
[[ "$response" == *"hello from web"* && "${response,,}" == *"x-from: ingress"* ]] \
  || { echo "failed: with the secret's credentials ($response)"; false; }
echo "ok: credentials from the secret, and headers"

# The status of Ingresses: proksi's address
cluster_ip="$(kubectl -n proksi get service proksi -o jsonpath='{.spec.clusterIP}')"
for _ in $(seq 1 60); do
  published="$(kubectl get ingress web -o jsonpath='{.status.loadBalancer.ingress[0].ip}')"
  [[ "$published" == "$cluster_ip" ]] && break
  sleep 1
done
[[ "$published" == "$cluster_ip" ]] || { echo "failed: status ($published, wanted $cluster_ip)"; false; }
echo "ok: the Ingress status has proksi's address"

# An Event on the Ingress that can't be served
for _ in $(seq 1 60); do
  events="$(kubectl get events --field-selector involvedObject.name=web-invalid -o jsonpath='{.items[*].reason}')"
  [[ "$events" == *"Invalid"* ]] && break
  sleep 1
done
[[ "$events" == *"Invalid"* ]] || { echo "failed: no event for web-invalid ($events)"; false; }
echo "ok: an Event for the invalid Ingress"

kubectl scale deployment/web --replicas=0
expect "no ready pods: 503" '[[ "$body" == *" 503" ]]'

kubectl scale deployment/web --replicas=2
expect "pods back" '[[ "$body" == *"hello from web"*" 200" ]]'

kubectl delete ingress web
expect "the Ingress removed: an unknown host" '[[ "$body" != *"hello from web"* ]]'
