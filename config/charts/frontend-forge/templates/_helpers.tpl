{{- define "frontend-forge.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "frontend-forge.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "frontend-forge.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service | quote }}
app.kubernetes.io/instance: {{ .Release.Name | quote }}
{{- with .Values.commonLabels }}
{{ toYaml . }}
{{- end }}
{{- end -}}

{{- define "frontend-forge.componentLabels" -}}
{{ include "frontend-forge.labels" .root }}
app.kubernetes.io/name: {{ .name | quote }}
app.kubernetes.io/component: {{ .component | quote }}
{{- end -}}

{{- define "frontend-forge.image" -}}
{{- if .tag -}}
{{- printf "%s:%s" .repository .tag -}}
{{- else -}}
{{- .repository -}}
{{- end -}}
{{- end -}}

{{- define "frontend-forge.controllerServiceAccountName" -}}
{{- default "frontend-forge-controller" .Values.frontendForgeController.serviceAccount.name -}}
{{- end -}}

{{- define "frontend-forge.runnerServiceAccountName" -}}
{{- default "frontend-forge-runner" .Values.frontendForgeRunner.serviceAccount.name -}}
{{- end -}}

{{- define "frontend-forge.extensionControllerServiceAccountName" -}}
{{- default "frontend-extension-controller" .Values.frontendExtensionController.serviceAccount.name -}}
{{- end -}}

{{- define "frontend-forge.extensionPackagerServiceAccountName" -}}
{{- default "frontend-forge-extension-packager" .Values.frontendForgeExtensionPackager.serviceAccount.name -}}
{{- end -}}

{{- define "frontend-forge.extensionPublisherServiceAccountName" -}}
{{- default "frontend-forge-extension-publisher" .Values.frontendForgeExtensionPublisher.serviceAccount.name -}}
{{- end -}}

{{- define "frontend-forge.extensionApiServiceAccountName" -}}
{{- default "frontend-forge-extension-api" .Values.frontendForgeExtensionApi.serviceAccount.name -}}
{{- end -}}

{{- define "frontend-forge.webhookCertgenServiceAccountName" -}}
{{- default "frontend-forge-webhook-certgen" .Values.webhook.certgen.serviceAccount.name -}}
{{- end -}}
