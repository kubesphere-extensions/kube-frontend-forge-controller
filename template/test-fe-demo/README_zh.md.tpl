{#- `fe_cr` contains the complete FrontendExtension CR object for future template extensions: fe_cr.metadata, fe_cr.spec, fe_cr.status. -#}
{{ extension_display_name }} 基于 KubeSphere 快速集成能力构建，通过 Kubernetes CRD 资源集成与页面集成两种模式，可根据业务场景快速扩展平台功能，为用户提供统一的使用与管理体验。

## 功能
{% for integration in integrations %}

### {{ loop.index }}：「{{ integration.title }}」（{{ integration.kind_label }}）

{% if integration.kind == "crdTable" -%}
通过 Kubernetes CRD（Custom Resource Definition）方式扩展平台资源，用户可在平台中直接查看、管理和操作这些自定义资源，并复用 KubeSphere 的权限体系对资源访问进行控制。
{% if integration.crd_resource %}

集成 CRD 资源：

* API Version：`{{ integration.crd_resource.group }}/{{ integration.crd_resource.version }}`
* Resource：`{{ integration.crd_resource.plural }}`
{% endif %}
{%- elif integration.kind == "iframe" -%}
通过 IFrame 方式嵌入第三方页面。
{% if integration.iframe_src %}

嵌入地址：

```text
{{ integration.iframe_src }}
```
{% endif %}
{%- endif %}

菜单入口：{{ integration.menu_entry_phrase }}
{% endfor %}

## 快速开始

扩展安装完成后，可在{{ all_menu_entry_phrase }}看到菜单 {{ top_menu_phrase }} 入口。
{% for integration in integrations %}

{{ loop.index }}. {{ integration.quick_start_text }}
{% endfor %}
