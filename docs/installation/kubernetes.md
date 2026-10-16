# Kubernetes

Proksi can run as an ingress controller: it serves the `Ingress` resources of its class, sending requests straight to the ready pods of each backend service. See [ADR 0005](adrs/0005-kubernetes-ingress.md) for the design.

## Installing

```bash
kubectl apply -f https://raw.githubusercontent.com/luizfonseca/proksi/main/deploy/kubernetes/proksi.yaml
```

This creates the `proksi` namespace with Proksi's `Deployment`, a `LoadBalancer` `Service` for ports 80 and 443, its permissions (read `Ingress`es, `IngressClass`es, `Service`s, `EndpointSlice`s and TLS `Secret`s), the `proksi` `IngressClass`, and a `ConfigMap` with Proksi's configuration:

```hcl
kubernetes {
  enabled = true
}
```

The `kubernetes` block takes:

| Attribute | Default | |
| --- | --- | --- |
| `enabled` | `false` | Watch the cluster |
| `ingress_class` | `"proksi"` | The class of the `Ingress`es to serve |
| `namespaces` | `[]` | Namespaces to watch (all of them by default) |

Outside a cluster (for development), Proksi uses your kubeconfig.

## Ingresses

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: web
spec:
  ingressClassName: proksi
  rules:
    - host: web.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: web
                port:
                  name: http
```

- Each `host` is a host in Proksi, and each path a location. `Prefix` matches the path and everything below it (`/api` matches `/api` and `/api/users`, not `/apis`), `Exact` only the path, and `ImplementationSpecific` uses Proksi's patterns (`/users/:id`, `/static/*`).
- Upstreams are the ready pods of the backend service, for the port it targets. They follow the pods as they scale or restart. A service without ready pods answers `503`.
- Rules of several `Ingress`es for the same host are merged. When two define the same path, the oldest `Ingress` keeps it and the other is logged.
- A rule without a `host`, and `defaultBackend`, are served for any other name (`host "*"`).
- `Ingress`es without a class are served when the `proksi` `IngressClass` has the annotation `ingressclass.kubernetes.io/is-default-class: "true"`.
- With the annotation `proksi.info/upstream: service`, the upstream is the service's own address (its cluster IP) instead of its pods, and Kubernetes balances the requests.

## TLS

Hosts are served over HTTPS. Hosts listed in an `Ingress`' `tls` use the certificate of its `Secret` (of type `kubernetes.io/tls`, in the same namespace), and pick up changes to it, e.g. renewals by cert-manager:

```yaml
spec:
  tls:
    - hosts: ["web.example.com"]
      secretName: web-example-com
```

Other hosts get certificates from Let's Encrypt (enable `lets_encrypt` in the `ConfigMap`). With the annotation `proksi.info/tls: "false"`, the hosts of an `Ingress` are plain HTTP.

With a shared store, a certificate from a `Secret` reaches every replica through the store.

Hosts can still come from Proksi's configuration file, docker labels or the store. When the same host comes from several places, the store wins over Kubernetes, which wins over docker labels and the configuration file.

## More than one replica

With more than one replica, use a shared store (Redis or Postgres) so certificates are ordered once and every replica serves them. With the memory store, Proksi logs a warning.

## Coming next

Plugins through annotations, the `Ingress` status, and a Helm chart.
