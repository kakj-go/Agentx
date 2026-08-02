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
The startup script waits for the database migration job. On a fresh database, open the web URL and complete the company setup form.

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
