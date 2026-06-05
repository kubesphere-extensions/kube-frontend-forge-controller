{#- `fe_cr` contains the complete FrontendExtension CR object for future template extensions: fe_cr.metadata, fe_cr.spec, fe_cr.status. -#}
{{ package_name }}

## 通用介绍

本扩展是基于平台快速集成能力构建的功能组件，可与平台无缝集成，为用户提供统一的使用与管理体验。
{% for integration in integrations %}

---

## 集成项 {{ loop.index }}：{{ integration.title }}（{{ integration.kind_label }}）

{% if integration.kind == "crdTable" -%}
通过 **CRD（云原生声明式扩展）** 方式作用于 {{ integration.placement_phrase }} 级别。
{% if integration.crd_resource %}

- `{{ integration.crd_resource.plural }}`（`{{ integration.crd_resource.group }}/{{ integration.crd_resource.version }}`）
{% endif %}
{%- elif integration.kind == "iframe" -%}
通过 **IFrame（前端页面级嵌入）** 方式嵌入外部页面。
{% if integration.iframe_src %}

- **嵌入地址**: `{{ integration.iframe_src }}`
{% endif %}
{%- endif %}
{% endfor %}
