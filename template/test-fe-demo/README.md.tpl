{#- `fe_cr` contains the complete FrontendExtension CR object for future template extensions: fe_cr.metadata, fe_cr.spec, fe_cr.status. -#}
{{ extension_display_name }} is built with KubeSphere rapid integration capabilities. It supports Kubernetes CRD resource integration and page integration, allowing platform features to be extended quickly for different business scenarios while providing a unified user and management experience.

## Features
{% for integration in integrations %}

### {{ loop.index }}. {{ integration.title }} ({{ integration.kind_label }})

{% if integration.kind == "crdTable" -%}
Extends platform resources through Kubernetes CRD (Custom Resource Definition). Users can view, manage, and operate these custom resources directly in the platform, and reuse KubeSphere's permission system to control resource access.
{% if integration.crd_resource %}

- **Integrated resource:** `{{ integration.crd_resource.plural }}.{{ integration.crd_resource.group }}`

{% endif %}

- **Menu entry:** {{ integration.menu_entry_phrase }}
{%- elif integration.kind == "iframe" -%}
Embeds a third-party page through IFrame.
{% if integration.iframe_src %}

- **Embed URL:** `{{ integration.iframe_src }}`

{% endif %}

- **Menu entry:** {{ integration.menu_entry_phrase }}
{%- endif %}
{% endfor %}

## Quick Start

After the extension is installed, the {{ top_menu_phrase }} menu entry is available in {{ all_menu_entry_phrase }}.
{% for integration in integrations %}

{{ loop.index }}. {{ integration.quick_start_text }}
{% endfor %}
