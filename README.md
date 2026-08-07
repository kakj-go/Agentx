# Local Kubernetes

## Prerequisites

- Docker Desktop with Kubernetes enabled
- kubectl
- PowerShell 7

## Build local images

    ./scripts/build-images.ps1

## Start all local services

    ./scripts/k8s-up.ps1

This starts the Agentx application processes, MySQL, Redis, ClickHouse, MinIO and the local Echo MCP service.
The startup script waits for the database migration job. On a fresh database, open the web URL and complete the company setup form.

M2.1 replaces the development-stage Tool and ZIP-only Skill schema. Existing local M2 data is not compatible; recreate the development MySQL PVC before testing this version:

    ./scripts/reset-dev-data.ps1

## Check status

    kubectl -n agentx get pods

## Open the web console

The local overlay exposes the Web service through Docker Desktop's local load balancer:

    http://127.0.0.1:8080

If the local Kubernetes runtime does not provide a load balancer, use:

    kubectl -n agentx port-forward service/web 18080:8080

Then open http://127.0.0.1:18080.

## Stop all local services

    ./scripts/k8s-down.ps1

## Optional LightRAG and Mem0 addons

    ./scripts/k8s-addons-up.ps1 -Addon all
    ./scripts/k8s-addons-status.ps1
    ./scripts/k8s-addons-down.ps1 -Addon all

Use Addon lightrag or mem0 to manage only one service. Set the model and embedding keys in the generated Kubernetes Secrets before testing real LightRAG queries or Mem0 writes; without keys, only deployment health is accepted.

## Kubernetes UI E2E

    ./scripts/e2e.ps1

The script creates a temporary agentx-e2e Namespace, deploys clean storage and Echo MCP, runs Playwright through the Web UI, saves reports under apps/e2e, and removes the Namespace. Use KeepNamespace only when debugging a failure.



1. 上下文和压缩帮我继续深度优化
2. 真实 Host 验收暂时没有这个环境，可以先忽略 host 验收
3. 继续完善
4. Plugin 暂时不考虑深入，界面还是保持开发中
5. Heartbeat 产品语义是啥意思讨论下
6. 文档同步一下

