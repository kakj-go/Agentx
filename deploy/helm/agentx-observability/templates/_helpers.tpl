{{- define "agentx.image" -}}
{{- $root := index . 0 -}}{{- $name := index . 1 -}}{{- $images := $root.Values.global.images -}}
{{- $prefix := $images.repositoryPrefix | default "" -}}
{{- $imageName := ternary $name (printf "%s%s" $prefix $name) (hasPrefix $prefix $name) -}}
{{- $repository := printf "%s/%s" (trimSuffix "/" $images.registry) $imageName -}}
{{- $digest := index ($images.digests | default dict) $name -}}
{{- if $digest -}}{{ printf "%s@%s" $repository $digest }}{{- else -}}{{ printf "%s:%s" $repository $images.tag }}{{- end -}}
{{- end -}}
{{- define "agentx.observability.caEnv" -}}
{{- if eq .Values.global.environment "production" }}
- { name: SSL_CERT_FILE, value: /etc/agentx-ca/clickhouse.pem }
- { name: AGENTX_OBSERVABILITY_REDIS_TLS_CA_PATH, value: /etc/agentx-ca/redis.pem }
- { name: AGENTX_OBSERVABILITY_S3_TLS_CA_PATH, value: /etc/agentx-ca/s3.pem }
{{- end }}
{{- end -}}
{{- define "agentx.observability.caMount" -}}
{{- if eq .Values.global.environment "production" }}
- { name: external-ca, mountPath: /etc/agentx-ca, readOnly: true }
{{- else }}
[]
{{- end }}
{{- end -}}
{{- define "agentx.observability.caVolume" -}}
{{- if eq .Values.global.environment "production" }}
- name: external-ca
  projected:
    sources:
      - secret: { name: {{ .Values.global.components.clickhouse.caSecretName }}, items: [{ key: ca.crt, path: clickhouse.pem }] }
      - secret: { name: {{ .Values.global.components.runtimeRedis.caSecretName }}, items: [{ key: ca.crt, path: redis.pem }] }
      - secret: { name: {{ .Values.global.components.objectStorage.caSecretName }}, items: [{ key: ca.crt, path: s3.pem }] }
{{- else }}
[]
{{- end }}
{{- end -}}
{{- define "agentx.observability.schemaGate" -}}
- name: wait-for-observability-schema
  image: curlimages/curl:8.12.1
  command: [sh, -ec]
  args: ['until test "$(curl --fail --silent {{ if eq .Values.global.environment "production" }}--cacert /etc/agentx-ca/clickhouse.pem{{ end }} --user "$AGENTX_CLICKHOUSE_USER:$AGENTX_CLICKHOUSE_PASSWORD" --data-binary "SELECT count() FROM observability_schema_migrations WHERE version=2" "${AGENTX_CLICKHOUSE_URL%/}/" 2>/dev/null)" -ge "1"; do sleep 2; done']
  env:
    - { name: AGENTX_CLICKHOUSE_URL, value: {{ .Values.global.components.clickhouse.url | quote }} }
    - { name: AGENTX_CLICKHOUSE_USER, value: {{ .Values.global.components.clickhouse.queryUser | quote }} }
    - name: AGENTX_CLICKHOUSE_PASSWORD
      valueFrom: { secretKeyRef: { name: {{ include "agentx.secret" (list . "observability" "observability") }}, key: AGENTX_CLICKHOUSE_QUERY_PASSWORD } }
  volumeMounts:
{{ include "agentx.observability.caMount" . | nindent 4 }}
  resources: { requests: { cpu: 10m, memory: 16Mi }, limits: { cpu: 100m, memory: 64Mi } }
  securityContext: { allowPrivilegeEscalation: false, runAsNonRoot: true, runAsUser: 101, runAsGroup: 102, capabilities: { drop: [ALL] } }
{{- end -}}
{{- define "agentx.secret" -}}
{{- $root := index . 0 -}}{{- $workload := index . 1 -}}{{- $plane := index . 2 -}}
{{- index ($root.Values.global.secrets.workloads | default dict) $workload | default (index $root.Values.global.secrets $plane) -}}
{{- end -}}
