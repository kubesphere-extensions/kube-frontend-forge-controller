use super::*;

pub(crate) fn transform_columns(columns: &[ColumnSpec]) -> Vec<Value> {
    columns
        .iter()
        .map(|col| {
            let mut payload = payload_object(col.render.payload.as_ref());
            if let Some(format) = &col.render.format {
                payload.insert("format".to_string(), json!(format));
            }
            if let Some(pattern) = &col.render.pattern {
                payload.insert("pattern".to_string(), json!(pattern));
            }
            if let Some(link) = &col.render.link {
                payload.insert("link".to_string(), json!(link));
            }

            let mut out = Map::new();
            out.insert("key".to_string(), json!(col.key));
            out.insert("title".to_string(), json!(col.title));
            out.insert(
                "render".to_string(),
                json!({
                  "type": render_type_str(col.render.type_),
                  "path": col.render.path,
                  "payload": Value::Object(payload),
                }),
            );
            if let Some(v) = col.enable_sorting {
                out.insert("enableSorting".to_string(), json!(v));
            }
            if let Some(v) = col.enable_hiding {
                out.insert("enableHiding".to_string(), json!(v));
            }
            Value::Object(out)
        })
        .collect()
}

pub(crate) fn payload_object(payload: Option<&Map<String, Value>>) -> Map<String, Value> {
    payload.cloned().unwrap_or_default()
}

pub(crate) const fn render_type_str(t: ColumnRenderType) -> &'static str {
    match t {
        ColumnRenderType::Text => "text",
        ColumnRenderType::Time => "time",
        ColumnRenderType::Link => "link",
    }
}
