{#- `fe_cr` contains the complete FrontendExtension CR object for future template extensions: fe_cr.metadata, fe_cr.spec, fe_cr.status. -#}
{{ package_name }}

## General Introduction

This extension is built with platform integration capabilities and integrates with the platform to provide a unified user and management experience.
{% for integration in integrations %}

---

## Integration Item {{ loop.index }}: {{ integration.title }} ({{ integration.kind_label }})

{% if integration.kind == "crdTable" -%}
Uses **CRD (cloud-native declarative extension)** integration at the {{ integration.placement_phrase }} scope.
{% if integration.crd_resource %}

- `{{ integration.crd_resource.plural }}` (`{{ integration.crd_resource.group }}/{{ integration.crd_resource.version }}`)
{% endif %}
{%- elif integration.kind == "iframe" -%}
Uses **IFrame (frontend page-level embedding)** integration to embed an external page.
{% if integration.iframe_src %}

- **Embed URL**: `{{ integration.iframe_src }}`
{% endif %}
{%- endif %}
{% endfor %}
