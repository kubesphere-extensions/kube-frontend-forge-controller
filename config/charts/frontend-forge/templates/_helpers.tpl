{{/*
Expand the name of the chart.
*/}}
{{- define "frontend-forge.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "frontend-forge.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "frontend-forge.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "frontend-forge.labels" -}}
helm.sh/chart: {{ include "frontend-forge.chart" . }}
{{ include "frontend-forge.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "frontend-forge.selectorLabels" -}}
app.kubernetes.io/name: {{ include "frontend-forge.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "frontend-forge.componentLabels" -}}
{{ include "frontend-forge.labels" .root }}
app.kubernetes.io/component: {{ .component }}
{{- end -}}

{{/*
Render an image from registry/repository/tag. If tag is empty and repository
already contains a tag, the repository is emitted unchanged.
*/}}
{{- define "frontend-forge.image" -}}
{{- $root := .root -}}
{{- $image := .image -}}
{{- $registry := default $image.registry $root.Values.global.imageRegistry -}}
{{- $repository := $image.repository -}}
{{- $tag := $image.tag -}}
{{- $base := ternary (printf "%s/%s" $registry $repository) $repository (ne $registry "") -}}
{{- if $tag -}}
{{- printf "%s:%s" $base $tag -}}
{{- else -}}
{{- $base -}}
{{- end -}}
{{- end }}

{{/*
Create the name of the runtime controller service account to use.
*/}}
{{- define "frontend-forge.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "frontend-forge.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Create the name of the runner service account to use.
*/}}
{{- define "frontend-forge.runnerServiceAccountName" -}}
{{- if .Values.runner.serviceAccount.create }}
{{- default (printf "%s-runner" (include "frontend-forge.fullname" .)) .Values.runner.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.runner.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "frontend-forge.extensionControllerServiceAccountName" -}}
{{- if .Values.extensionController.serviceAccount.create }}
{{- default (printf "%s-extension-controller" (include "frontend-forge.fullname" .)) .Values.extensionController.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.extensionController.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "frontend-forge.extensionPackagerServiceAccountName" -}}
{{- if .Values.extensionPackager.serviceAccount.create }}
{{- default (printf "%s-extension-packager" (include "frontend-forge.fullname" .)) .Values.extensionPackager.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.extensionPackager.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "frontend-forge.extensionPublisherServiceAccountName" -}}
{{- if .Values.extensionPublisher.serviceAccount.create }}
{{- default (printf "%s-extension-publisher" (include "frontend-forge.fullname" .)) .Values.extensionPublisher.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.extensionPublisher.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "frontend-forge.extensionApiServiceAccountName" -}}
{{- if .Values.extensionApi.serviceAccount.create }}
{{- default (printf "%s-extension-api" (include "frontend-forge.fullname" .)) .Values.extensionApi.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.extensionApi.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "frontend-forge.extensionApiServiceName" -}}
{{- printf "%s-extension-api" (include "frontend-forge.fullname" .) -}}
{{- end }}

{{- define "frontend-forge.extensionApiAPIServiceName" -}}
{{- $group := include "frontend-forge.extensionApiAPIServiceGroup" . -}}
{{- $version := include "frontend-forge.extensionApiAPIServiceVersion" . -}}
{{- default (printf "%s.%s" $version $group) .Values.extensionApi.apiService.name -}}
{{- end }}

{{- define "frontend-forge.extensionApiAPIServiceGroup" -}}
{{- default "frontend-forge-api.kubesphere.io" .Values.extensionApi.apiService.group -}}
{{- end }}

{{- define "frontend-forge.extensionApiAPIServiceVersion" -}}
{{- default "v1alpha1" .Values.extensionApi.apiService.version -}}
{{- end }}

{{- define "frontend-forge.extensionApiAPIServiceURL" -}}
{{- default (printf "http://%s.%s.svc:%v" (include "frontend-forge.extensionApiServiceName" .) .Release.Namespace (.Values.extensionApi.service.port | int)) .Values.extensionApi.apiService.url -}}
{{- end }}

{{- define "frontend-forge.webhookCertgenServiceAccountName" -}}
{{- if .Values.webhook.certgen.serviceAccount.create }}
{{- default (printf "%s-webhook-certgen" (include "frontend-forge.fullname" .)) .Values.webhook.certgen.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.webhook.certgen.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "frontend-forge.webhookServiceName" -}}
{{- default (printf "%s-webhook" (include "frontend-forge.fullname" .)) .Values.webhook.service.name -}}
{{- end }}

{{- define "frontend-forge.webhookSecretName" -}}
{{- default (printf "%s-webhook-tls" (include "frontend-forge.fullname" .)) .Values.webhook.secretName -}}
{{- end }}

{{- define "frontend-forge.validatingWebhookName" -}}
{{- default (printf "%s-validating-webhook" (include "frontend-forge.fullname" .)) .Values.webhook.validatingWebhookName -}}
{{- end }}

{{- define "frontend-forge.buildServiceName" -}}
{{- default (include "frontend-forge.fullname" .) .Values.buildService.name -}}
{{- end }}
