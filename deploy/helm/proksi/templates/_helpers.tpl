{{- define "proksi.name" -}}
{{- default .Release.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "proksi.labels" -}}
app.kubernetes.io/name: proksi
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
{{- end -}}

{{- define "proksi.selector" -}}
app.kubernetes.io/name: proksi
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}
