# Specification documents for `Proksi`

## Table of Contents

- [Overview](#overview)
- [Store](#store)
  - [DragonflyDB / Redis](#dragonflydb--redis)

## Overview
This document provides a comprehensive specification for the proxy server, covering its architecture, design principles, and implementation details. It serves as a reference for developers working on the project and ensures consistency across different components.

## Store

The store is responsible for managing data persistence such as **certificate** storage, key management, and data replication between multiple nodes/replicas of `proksi`.

### DragonflyDB / Redis
You can run a local instance of DragonflyDB or Redis using Docker:

```bash
docker run -p 6379:6379 --ulimit memlock=-1 docker.dragonflydb.io/dragonflydb/dragonfly
```

This can be used for development and testing purposes.

### Store keys

The following keys are used by the store to persist `Proksi` configuration data:

| Key | Description | Content |
| --- | ----------- | ------- |
|`proksi:certs:<domain>` | Stores the certificate for a given domain | `{ "key": "...", "leaf": "...", "chain": "..." }` |
|`proksi:challenges:<domain>` | Stores the challenge for a given domain. (Ttl = 600 seconds) | `<challengeId>` |
|`proksi:upstream:<host>` | Stores the routing information for a given host that has multiple upstreams | `{ "upstreams": []}` |
|`proksi:config` | All non routing `proksi` configuration data | `{ "lets_encrypt": {}, "logging": {}}` |
