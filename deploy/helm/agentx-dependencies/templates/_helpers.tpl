{{- define "agentx.image" -}}
{{- $root := index . 0 -}}{{- $name := index . 1 -}}{{- $images := $root.Values.global.images -}}
{{- $prefix := $images.repositoryPrefix | default "" -}}
{{- $imageName := ternary $name (printf "%s%s" $prefix $name) (hasPrefix $prefix $name) -}}
{{- $repository := printf "%s/%s" (trimSuffix "/" $images.registry) $imageName -}}
{{- $digest := index ($images.digests | default dict) $name -}}
{{- if $digest -}}{{ printf "%s@%s" $repository $digest }}{{- else -}}{{ printf "%s:%s" $repository $images.tag }}{{- end -}}
{{- end -}}
