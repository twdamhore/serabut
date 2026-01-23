{{/*
Expand the name of the chart.
*/}}
{{- define "serabut.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "serabut.fullname" -}}
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
{{- define "serabut.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "serabut.labels" -}}
helm.sh/chart: {{ include "serabut.chart" . }}
{{ include "serabut.selectorLabels" . }}
app.kubernetes.io/version: {{ .Values.appVersion | default .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "serabut.selectorLabels" -}}
app.kubernetes.io/name: {{ include "serabut.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
NetworkAttachmentDefinition name
*/}}
{{- define "serabut.nadName" -}}
{{- printf "%s-%s" (include "serabut.fullname" .) .Values.network.interface }}
{{- end }}
