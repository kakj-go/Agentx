# Local Kubernetes

## Prerequisites

- Docker Desktop with Kubernetes enabled
- kubectl
- PowerShell 7

## Build local images

    ./scripts/build-images.ps1

## Start all local services

    ./scripts/k8s-up.ps1

This starts the Agentx application processes, MySQL, Redis, ClickHouse and MinIO.

## Check status

    kubectl -n agentx get pods

## Open the web console

    kubectl -n agentx port-forward service/web 8080:80

Open http://127.0.0.1:8080.

## Stop all local services

    ./scripts/k8s-down.ps1

