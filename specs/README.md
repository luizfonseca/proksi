# Specification documents for the proxy server

## Table of Contents

- [Overview](#overview)
- [Store](#store)
  - [DragonflyDB / Redis](#dragonflydb--redis)

## Overview
This document provides a comprehensive specification for the proxy server, covering its architecture, design principles, and implementation details. It serves as a reference for developers working on the project and ensures consistency across different components.

## Store

The store is responsible for managing data persistence such as **certificate** storage, key management, and data replication between multiple nodes/replicas of `proksi`.

### DragonflyDB / Redis
```bash
docker run -p 6379:6379 --ulimit memlock=-1 docker.dragonflydb.io/dragonflydb/dragonfly
```
