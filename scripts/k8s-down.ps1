$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path -Parent $PSScriptRoot

kubectl delete -k "$root/deploy/k8s/overlays/local"
